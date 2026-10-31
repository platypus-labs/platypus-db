//! What agents may run: the connection's agent access, then the statement itself, checked
//! with the same analysis the app runs before a statement (`platypus_sql::analyze`).

use platypus_core::{DriverKind, TxEffect, classify};
use platypus_sql::{Dialect, Lexer, StatementKind, Token, TokenKind};
pub use platypus_store::AgentAccess;
use platypus_store::SettingsStore;

use crate::error::{AgentError, Result};
use crate::ident;

/// What the user's Safety setting for `UPDATE` or `DELETE` without `WHERE` (and `TRUNCATE`)
/// means for agents, who cannot answer the app's confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnfilteredRule {
    /// "Run without asking".
    Allow,
    /// "Ask before running" or "Never run".
    Refuse,
}

impl UnfilteredRule {
    /// From `safety.unfiltered` in the app's settings: only `off` lets such statements run.
    #[must_use]
    pub fn from_settings(settings: Option<&SettingsStore>) -> Self {
        match settings
            .and_then(|s| s.get::<String>("safety.unfiltered"))
            .as_deref()
        {
            Some("off") => Self::Allow,
            _ => Self::Refuse,
        }
    }
}

/// What reaches outside a read-only transaction in a dialect, or runs SQL text the
/// statement check cannot see. Names are lower case; a trailing `*` matches any end.
struct Outside {
    /// Names that do it wherever they stand.
    words: &'static [&'static str],
    /// Names that do it only when called, `name(…)`: as plain words they are common
    /// column names (`url`, `file`, `query`).
    calls: &'static [&'static str],
    /// Names the patterns above match that only read.
    reads: &'static [&'static str],
}

/// PostgreSQL functions that act outside the transaction (other sessions, the server's
/// configuration, files and statistics, replication, notifications, session locks), which a `READ
/// ONLY` transaction lets run, or run a query given as text (`query_to_xml`, `ts_stat`).
/// Families: `dblink_exec`, `pg_advisory_lock`, adminpack's `pg_file_write`…
const POSTGRES_OUTSIDE: Outside = Outside {
    words: &[
        "pg_terminate_backend",
        "pg_cancel_backend",
        "pg_reload_conf",
        "pg_rotate_logfile",
        "pg_promote",
        "lo_import",
        "lo_export",
        "pg_notify",
        "dblink*",
        "pg_advisory_*",
        "pg_try_advisory_*",
        "pg_file_*",
        "pg_read_file",
        "pg_read_binary_file",
        "pg_stat_file",
        "pg_ls_*",
        "pg_create_*",
        "pg_drop_replication_slot",
        "pg_replication_*",
        "pg_logical_*",
        "pg_switch_wal",
        "pg_backup_*",
        "pg_start_backup",
        "pg_stop_backup",
        "pg_log_backend_memory_contexts",
        "pg_log_standby_snapshot",
        "pg_wal_replay_*",
        "pg_sync_replication_slots",
        "pg_stat_reset*",
        "pg_stat_statements_reset",
        "query_to_xml*",
        "ts_stat",
    ],
    calls: &[],
    reads: &[],
};
/// MySQL and MariaDB read and write the server's files whatever the transaction, and keep
/// named locks (and `lib_mysqludf_sys` commands) past it.
const MYSQL_OUTSIDE: Outside = Outside {
    words: &[
        "load_file",
        "outfile",
        "dumpfile",
        "get_lock",
        "release_lock",
        "release_all_locks",
        "sys_exec",
        "sys_eval",
    ],
    calls: &[],
    reads: &[],
};
/// Snowflake's `SYSTEM$` functions run inside a `SELECT` but include ones that act on other
/// sessions and queries (`SYSTEM$CANCEL_QUERY`, `SYSTEM$ABORT_SESSION`) or on objects
/// (`SYSTEM$PIPE_FORCE_RESUME`): agents get none of them.
const SNOWFLAKE_OUTSIDE: Outside = Outside {
    words: &["system$*"],
    calls: &[],
    reads: &[],
};
/// SQLite extension loading and the shell's file functions.
const SQLITE_OUTSIDE: Outside = Outside {
    words: &[],
    calls: &[
        "load_extension",
        "readfile",
        "writefile",
        "edit",
        "fts3_tokenizer",
    ],
    reads: &[],
};
/// DuckDB runs inside the app, with the user's rights: its table functions read any local
/// file (or URL), and `query()` runs SQL given as text.
const DUCKDB_OUTSIDE: Outside = Outside {
    words: &[],
    calls: &[
        "read_*",
        "parquet_*",
        "sniff_csv",
        "glob",
        "getenv",
        "query",
        "query_table",
        "sqlite_*",
        "postgres_*",
        "mysql_*",
        "iceberg_*",
        "delta_*",
        "st_read",
        "json_execute_serialized_sql",
        "load_aws_credentials",
    ],
    reads: &[],
};
/// The ClickHouse table functions that read only the server's own tables or make up rows:
/// every other one in a `FROM` list (`url`, `s3`, `deltaLake`, `iceberg`, `fileCluster`,
/// `ytsaurus`…, and the ones later versions add) may reach other servers or the server's
/// files, so a read-only agent's statement cannot call it. The driver also runs read-only
/// sessions with `readonly = 1`, which refuses them on the server.
const CLICKHOUSE_LOCAL_TABLE_FUNCTIONS: &[&str] = &[
    "numbers",
    "numbers_mt",
    "zeros",
    "zeros_mt",
    "generate_series",
    "generateseries",
    "generaterandom",
    "values",
    "view",
    "viewifpermitted",
    "null",
    "format",
    "merge",
    "dictionary",
    "loop",
];
/// ClickHouse functions that read the server's files, other servers and URLs, or run
/// scripts (`executable`), wherever they are called (`file()` is a plain function too).
const CLICKHOUSE_OUTSIDE: Outside = Outside {
    words: &[],
    calls: &[
        "file",
        "url",
        "urlcluster",
        "s3",
        "s3cluster",
        "gcs",
        "hdfs",
        "hdfscluster",
        "azureblobstorage",
        "remote",
        "remotesecure",
        "cluster",
        "clusterallreplicas",
        "mysql",
        "postgresql",
        "mongodb",
        "redis",
        "jdbc",
        "odbc",
        "sqlite",
        "executable",
    ],
    reads: &[],
};
/// Oracle's packages reach files, the network and other sessions (`UTL_FILE`, `UTL_HTTP`,
/// `DBMS_PIPE`), or run SQL or PL/SQL given as text (`DBMS_XMLQUERY`, `DBMS_XMLGEN`), and
/// Oracle runs agents' statements in no read-only transaction.
const ORACLE_OUTSIDE: Outside = Outside {
    words: &[
        "dbms_*",
        "utl_*",
        "owa_*",
        "ctx_*",
        "htp",
        "htf",
        "httpuritype",
    ],
    calls: &[],
    reads: &[
        "dbms_lob",
        "dbms_random",
        "dbms_metadata",
        "dbms_utility",
        "dbms_xplan",
        "dbms_assert",
    ],
};
/// Trino connectors' pass-through table functions run their SQL on the source database.
const TRINO_OUTSIDE: Outside = Outside {
    words: &[],
    calls: &["query", "raw_query"],
    reads: &[],
};
/// Spark calls Java methods.
const DATABRICKS_OUTSIDE: Outside = Outside {
    words: &[],
    calls: &["reflect", "try_reflect", "java_method"],
    reads: &[],
};
/// `EXTERNAL_QUERY` runs its SQL text on an external database.
const BIGQUERY_OUTSIDE: Outside = Outside {
    words: &[],
    calls: &["external_query"],
    reads: &[],
};
const NOTHING_OUTSIDE: Outside = Outside {
    words: &[],
    calls: &[],
    reads: &[],
};

impl Outside {
    fn of(dialect: Dialect) -> &'static Self {
        match dialect {
            Dialect::Postgres => &POSTGRES_OUTSIDE,
            Dialect::MySql => &MYSQL_OUTSIDE,
            Dialect::Snowflake => &SNOWFLAKE_OUTSIDE,
            Dialect::Sqlite => &SQLITE_OUTSIDE,
            Dialect::DuckDb => &DUCKDB_OUTSIDE,
            Dialect::ClickHouse => &CLICKHOUSE_OUTSIDE,
            Dialect::Oracle => &ORACLE_OUTSIDE,
            Dialect::Ansi => &TRINO_OUTSIDE,
            Dialect::Databricks => &DATABRICKS_OUTSIDE,
            Dialect::BigQuery => &BIGQUERY_OUTSIDE,
            // Transact-SQL's (`OPENROWSET`, `xp_cmdshell`…) are no query to the analysis.
            // `CALL`, which runs procedures such as Trino's `system.runtime.kill_query`, is
            // never a query either.
            Dialect::SqlServer | Dialect::Db2 => &NOTHING_OUTSIDE,
        }
    }

    /// Whether `name` (lower case), called when `call`, reaches outside.
    fn reaches(&self, name: &str, call: bool) -> bool {
        let hit = |patterns: &[&str]| {
            patterns.iter().any(|p| match p.strip_suffix('*') {
                Some(prefix) => name.starts_with(prefix),
                None => name == *p,
            })
        };
        (hit(self.words) || (call && hit(self.calls))) && !self.reads.contains(&name)
    }
}

/// A DuckDB table read from a file or URL named in the `FROM` list: `FROM 'x.csv'`,
/// `FROM "x.parquet"`, `JOIN 'https://…'`.
fn duckdb_file_scan(src: &str, tokens: &[Token]) -> Option<usize> {
    let mut in_from = false;
    (1..tokens.len()).find(|&i| {
        let prev = &tokens[i - 1];
        let prev_text = prev.text(src);
        let prev_word = (prev.kind == TokenKind::Word).then(|| prev_text.to_ascii_uppercase());
        let opens_from = matches!(prev_word.as_deref(), Some("FROM" | "JOIN"));
        if opens_from {
            in_from = true;
        } else if matches!(
            prev_word.as_deref(),
            Some(
                "SELECT" | "WHERE" | "GROUP" | "HAVING" | "ORDER" | "WINDOW" | "QUALIFY" | "LIMIT"
            )
        ) {
            in_from = false;
        }
        let starts_table =
            opens_from || (in_from && prev.kind == TokenKind::Symbol && prev_text == ",");
        let t = &tokens[i];
        let text = t.text(src);
        starts_table
            && (t.kind == TokenKind::String
                || (t.kind == TokenKind::QuotedIdent && text.contains(['.', '/', '\\', ':'])))
    })
}

/// Which of `tokens` stand where ClickHouse reads a table: after `FROM`, `JOIN` (not
/// `ARRAY JOIN`, whose argument is an array) and `DESCRIBE [TABLE]`, and after each `,` of a
/// `FROM` list, in subqueries too. The `FROM` inside a function's parentheses
/// (`extract(DAY FROM d)`, `trim(BOTH ' ' FROM s)`) is none.
fn clickhouse_table_starts(src: &str, tokens: &[Token]) -> Vec<bool> {
    let word = |i: usize| {
        tokens
            .get(i)
            .filter(|t| t.kind == TokenKind::Word)
            .map(|t| t.text(src).to_ascii_uppercase())
    };
    let symbol = |i: usize, s: &str| {
        tokens
            .get(i)
            .is_some_and(|t| t.kind == TokenKind::Symbol && t.text(src) == s)
    };
    // Per level of parentheses: whether its `FROM` reads tables, and whether a `FROM` list
    // is being read.
    let mut levels: Vec<(bool, bool)> = vec![(true, false)];
    let mut starts = Vec::with_capacity(tokens.len());
    for i in 0..tokens.len() {
        let prev = i.checked_sub(1);
        let prev_word = prev.and_then(word);
        let array_join = prev_word.as_deref() == Some("JOIN")
            && i.checked_sub(2).and_then(word).as_deref() == Some("ARRAY");
        // `a IS [NOT] DISTINCT FROM b` compares: what follows is no table.
        let compares = i.checked_sub(2).and_then(word).as_deref() == Some("DISTINCT");
        let (tables, in_from) = *levels.last().unwrap_or(&(true, false));
        starts.push(match prev_word.as_deref() {
            Some("FROM") => tables && !compares,
            Some("JOIN") => !array_join,
            Some("DESCRIBE" | "DESC" | "TABLE") => true,
            _ => prev.is_some_and(|p| symbol(p, ",")) && in_from,
        });
        if symbol(i, "(") {
            let call =
                prev_word.is_some() && !matches!(word(i + 1).as_deref(), Some("SELECT" | "WITH"));
            let keyword = matches!(
                prev_word.as_deref(),
                Some("IN" | "EXISTS" | "AS" | "FROM" | "JOIN")
            );
            levels.push((!call || keyword, false));
        } else if symbol(i, ")") {
            if levels.len() > 1 {
                levels.pop();
            }
        } else if let Some(w) = word(i)
            && let Some(level) = levels.last_mut()
        {
            match w.as_str() {
                "FROM" => level.1 = level.0 && prev_word.as_deref() != Some("DISTINCT"),
                "JOIN" => level.1 = prev_word.as_deref() != Some("ARRAY"),
                "SELECT" | "WHERE" | "PREWHERE" | "GROUP" | "HAVING" | "ORDER" | "LIMIT"
                | "SETTINGS" | "FORMAT" | "UNION" | "WINDOW" | "QUALIFY" | "ARRAY" | "ON"
                | "USING" => level.1 = false,
                _ => {}
            }
        }
    }
    starts
}

/// The first name in `statement` that reaches outside a read-only transaction in
/// `dialect`, however the server reads the statement and decodes its quoted names: what a
/// read-only connection or agent may not run, though the statement only reads.
#[must_use]
pub fn outside_effect(statement: &str, dialect: Dialect) -> Option<String> {
    let outside = Outside::of(dialect);
    Lexer::readings(statement, dialect)
        .into_iter()
        .find_map(|lexer| {
            let tokens: Vec<Token> = lexer
                .filter(|t| !t.is_trivia() && t.kind != TokenKind::ExecComment)
                .collect();
            if dialect == Dialect::DuckDb
                && let Some(i) = duckdb_file_scan(statement, &tokens)
            {
                return Some(tokens[i].text(statement).to_owned());
            }
            let table_starts = if dialect == Dialect::ClickHouse {
                clickhouse_table_starts(statement, &tokens)
            } else {
                Vec::new()
            };
            tokens.iter().enumerate().find_map(|(i, t)| {
                let (names, last) = match t.kind {
                    TokenKind::Word => (vec![t.text(statement).to_owned()], i),
                    TokenKind::QuotedIdent => {
                        let decoded = ident::decode(statement, &tokens, i, dialect);
                        if decoded.undecodable {
                            return Some(t.text(statement).to_owned());
                        }
                        (decoded.names, decoded.last)
                    }
                    _ => return None,
                };
                let call = tokens
                    .get(last + 1)
                    .is_some_and(|n| n.kind == TokenKind::Symbol && n.text(statement) == "(");
                let table_function = call && table_starts.get(i).copied().unwrap_or(false);
                names.into_iter().find(|name| {
                    let lower = name.to_ascii_lowercase();
                    outside.reaches(&lower, call)
                        || (table_function
                            && !CLICKHOUSE_LOCAL_TABLE_FUNCTIONS.contains(&lower.as_str()))
                })
            })
        })
}

/// Whether `statement`, one statement as [`check`] or [`check_mongo`] returned it, may change
/// something: what the user is asked before it runs.
#[must_use]
pub fn changes(statement: &str, driver: DriverKind) -> bool {
    if driver == DriverKind::Mongo {
        use platypus_driver_mongo::shell::{self, StatementKind};
        return shell::parse(statement).is_ok_and(|c| shell::kind(&c) != StatementKind::Query);
    }
    if driver == DriverKind::Redis {
        use platypus_driver_redis::command::{self, Kind};
        return command::parse(statement).map_or(true, |c| c.kind() != Kind::Query);
    }
    if driver == DriverKind::Elasticsearch {
        use platypus_driver_search::command::{self, Kind};
        return command::parse(statement).map_or(true, |c| c.kind() != Kind::Query);
    }
    platypus_sql::analyze(statement, Dialect::from(driver)).kind != StatementKind::Query
}

/// The single statement of `sql`, if agents may run it on `connection` with `access`.
pub fn check<'a>(
    sql: &'a str,
    dialect: Dialect,
    access: AgentAccess,
    unfiltered: UnfilteredRule,
    connection: &str,
) -> Result<&'a str> {
    let statements = platypus_sql::split(sql, dialect);
    let statement = match statements.as_slice() {
        [] => return Err(AgentError::Invalid("there is no SQL to run".to_owned())),
        [one] => one.text(sql),
        many => {
            return Err(AgentError::Invalid(format!(
                "each call runs one statement at a time and this SQL has {}: send them in separate calls",
                many.len()
            )));
        }
    };
    check_statement(statement, dialect, access, unfiltered, connection)
}

/// [`check`] for CQL (Cassandra, `ScyllaDB`): the script is split as CQL splits it, a
/// `BEGIN BATCH ... APPLY BATCH` being one statement, and the statement is read with the
/// standard SQL analysis, which finds `TRUNCATE` and the writes that name no row. A batch is
/// refused: its statements would escape the check one by one.
pub fn check_cql<'a>(
    sql: &'a str,
    access: AgentAccess,
    unfiltered: UnfilteredRule,
    connection: &str,
) -> Result<&'a str> {
    use platypus_driver_cassandra::cql;

    let statement = match cql::split(sql).as_slice() {
        [] => return Err(AgentError::Invalid("there is no CQL to run".to_owned())),
        [one] => &sql[one.clone()],
        many => {
            return Err(AgentError::Invalid(format!(
                "each call runs one statement at a time and this CQL has {}: send them in separate calls",
                many.len()
            )));
        }
    };
    if access != AgentAccess::Off && cql::first_word(statement) == "BEGIN" {
        return Err(AgentError::Denied(
            "batches are not available to agents: run the statements one at a time".to_owned(),
        ));
    }
    // Judged on the text as the SQL lexer reads CQL (same offsets), but the statement that runs
    // is the original.
    check_statement(
        &cql::for_analysis(statement),
        Dialect::Ansi,
        access,
        unfiltered,
        connection,
    )
    .map(|_| statement)
}

/// [`check`] for the one `statement` it found.
fn check_statement<'a>(
    statement: &'a str,
    dialect: Dialect,
    access: AgentAccess,
    unfiltered: UnfilteredRule,
    connection: &str,
) -> Result<&'a str> {
    let analysis = platypus_sql::analyze(statement, dialect);
    match (access, analysis.kind) {
        (AgentAccess::Off, _) => Err(AgentError::NotFound(format!(
            "`{connection}` is not available to agents"
        ))),
        (_, StatementKind::Command) => Err(AgentError::Denied(
            "client commands such as `@set` belong to PlatypusDB's editor: send plain SQL"
                .to_owned(),
        )),
        (AgentAccess::ReadOnly, StatementKind::Query) => match outside_effect(statement, dialect) {
            Some(word) => Err(AgentError::Denied(format!(
                "`{connection}` is read-only for agents, and `{word}` acts outside the \
                 transaction or on the server's files, which a read-only transaction does not stop"
            ))),
            None => Ok(statement),
        },
        (AgentAccess::ReadOnly, kind) => Err(AgentError::Denied(format!(
            "`{connection}` is read-only for agents: only statements that change nothing may run, \
             and this one {}. The user can allow agents to write in PlatypusDB's connection \
             settings (Agent access), except on production and read-only connections",
            match kind {
                StatementKind::Dml => "changes data",
                StatementKind::Ddl => "changes the schema or permissions",
                _ => "changes the session or its transaction",
            }
        ))),
        // Also anywhere in a Transact-SQL batch or after a MySQL `DELIMITER`.
        (AgentAccess::ReadWrite, _)
            if analysis.transaction
                || matches!(classify(statement), TxEffect::Begin | TxEffect::End) =>
        {
            Err(AgentError::Denied(
                "each statement an agent runs commits on its own: BEGIN, COMMIT and ROLLBACK \
                 are not available"
                    .to_owned(),
            ))
        }
        (AgentAccess::ReadWrite, _)
            if !analysis.unfiltered.is_empty() && unfiltered == UnfilteredRule::Refuse =>
        {
            let tables: Vec<String> = analysis
                .unfiltered
                .iter()
                .map(|u| match &u.table.schema {
                    Some(schema) => format!("`{schema}.{}`", u.table.name),
                    None => format!("`{}`", u.table.name),
                })
                .collect();
            Err(AgentError::Denied(format!(
                "this statement changes every row of {}: add a WHERE. PlatypusDB's Safety \
                 settings ask before UPDATE or DELETE without WHERE and TRUNCATE, and agents \
                 cannot answer",
                tables.join(", ")
            )))
        }
        (AgentAccess::ReadWrite, _) => Ok(statement),
    }
}

/// [`check`] for MongoDB, whose statements are mongosh: checked with the editor's analysis
/// of them ([`platypus_driver_mongo::shell`]). There is no transaction to run them in, so
/// read-only access lets only the statements that read run.
pub fn check_mongo<'a>(
    sql: &'a str,
    access: AgentAccess,
    unfiltered: UnfilteredRule,
    connection: &str,
) -> Result<&'a str> {
    use platypus_driver_mongo::shell::{self, Command, StatementKind};

    let statement = match shell::split(sql).as_slice() {
        [] => {
            return Err(AgentError::Invalid(
                "there is no statement to run".to_owned(),
            ));
        }
        [one] => &sql[one.clone()],
        many => {
            return Err(AgentError::Invalid(format!(
                "each call runs one statement at a time and this script has {}: send them in separate calls",
                many.len()
            )));
        }
    };
    if access == AgentAccess::Off {
        return Err(AgentError::NotFound(format!(
            "`{connection}` is not available to agents"
        )));
    }
    let command = shell::parse(statement).map_err(|e| AgentError::Invalid(e.message))?;
    match (access, shell::kind(&command)) {
        // `use` would change the database of later statements on the shared connection.
        (AgentAccess::ReadOnly, StatementKind::Query) if !matches!(command, Command::Use(_)) => {
            Ok(statement)
        }
        (AgentAccess::ReadOnly, kind) => Err(AgentError::Denied(format!(
            "`{connection}` is read-only for agents: only statements that change nothing may run, \
             and this one {}. The user can allow agents to write in PlatypusDB's connection \
             settings (Agent access), except on production and read-only connections",
            match kind {
                StatementKind::Dml => "changes documents",
                StatementKind::Ddl => "changes collections or indexes",
                StatementKind::Other => "runs a database command, which can change anything",
                StatementKind::Query => "changes the session's database",
            }
        ))),
        _ => match (shell::unfiltered(&command), unfiltered) {
            (Some((db, name)), UnfilteredRule::Refuse) => Err(AgentError::Denied(format!(
                "this statement changes every document of `{}`: add a filter. PlatypusDB's \
                 Safety settings ask before such changes, and agents cannot answer",
                db.map_or_else(|| name.to_owned(), |db| format!("{db}.{name}"))
            ))),
            _ => Ok(statement),
        },
    }
}

/// [`check`] for Redis, whose statements are commands, one per line: checked with the
/// editor's analysis of them ([`platypus_driver_redis::command`]). There is no transaction to
/// run them in, so read-only access lets only the commands that read run.
pub fn check_redis<'a>(
    sql: &'a str,
    access: AgentAccess,
    unfiltered: UnfilteredRule,
    connection: &str,
) -> Result<&'a str> {
    use platypus_driver_redis::command::{self, Command, Kind};

    let statement = match command::split(sql).as_slice() {
        [] => return Err(AgentError::Invalid("there is no command to run".to_owned())),
        [one] => &sql[one.clone()],
        many => {
            return Err(AgentError::Invalid(format!(
                "each call runs one command at a time and this script has {}: send them in separate calls",
                many.len()
            )));
        }
    };
    if access == AgentAccess::Off {
        return Err(AgentError::NotFound(format!(
            "`{connection}` is not available to agents"
        )));
    }
    let parsed = command::parse(statement).map_err(AgentError::Invalid)?;
    // `SELECT` and `MULTI` would change what later calls on the shared session do.
    let session = matches!(
        parsed,
        Command::Select(_) | Command::Multi | Command::Exec | Command::Discard
    );
    match (access, parsed.kind()) {
        (AgentAccess::ReadOnly, Kind::Query) if !session => Ok(statement),
        (AgentAccess::ReadOnly, _) if parsed.reveals_secrets() => Err(AgentError::Denied(format!(
            "`{connection}` is read-only for agents, and `{statement}` shows the server's credentials \
                 (passwords or their hashes): ask for a parameter that holds none, e.g. \
                 `CONFIG GET maxmemory`, or `ACL USERS` and `ACL WHOAMI` for the users"
        ))),
        (AgentAccess::ReadOnly, kind) => Err(AgentError::Denied(format!(
            "`{connection}` is read-only for agents: only commands that change nothing may run, \
             and this one {}. The user can allow agents to write in PlatypusDB's connection \
             settings (Agent access), except on production and read-only connections",
            match kind {
                Kind::Dml => "changes keys",
                Kind::Ddl => "empties databases",
                Kind::Other => "changes the server",
                Kind::Query => "changes the session's database or transaction",
            }
        ))),
        _ if session => Err(AgentError::Invalid(format!(
            "{} changes the shared session: name the database in BROWSE (`BROWSE db1.hash`) instead",
            parsed.name()
        ))),
        (_, Kind::Ddl) if unfiltered == UnfilteredRule::Refuse => Err(AgentError::Denied(format!(
            "{} deletes every key: PlatypusDB's Safety settings ask before such changes, and \
             agents cannot answer",
            parsed.name()
        ))),
        _ => Ok(statement),
    }
}

/// [`check`] for Elasticsearch and `OpenSearch`, whose statements are console requests and SQL,
/// one at a time: checked with the editor's analysis of them
/// ([`platypus_driver_search::command`]). There is no transaction to run them in, so
/// read-only access lets only the requests that read run.
pub fn check_search<'a>(
    sql: &'a str,
    access: AgentAccess,
    unfiltered: UnfilteredRule,
    connection: &str,
) -> Result<&'a str> {
    use platypus_driver_search::command::{self, Kind};

    let statement = match command::split(sql).as_slice() {
        [] => return Err(AgentError::Invalid("there is no request to run".to_owned())),
        [one] => &sql[one.clone()],
        many => {
            return Err(AgentError::Invalid(format!(
                "each call runs one request at a time and this script has {}: send them in separate calls",
                many.len()
            )));
        }
    };
    if access == AgentAccess::Off {
        return Err(AgentError::NotFound(format!(
            "`{connection}` is not available to agents"
        )));
    }
    let parsed = command::parse(statement).map_err(AgentError::Invalid)?;
    match (access, parsed.kind()) {
        (AgentAccess::ReadOnly, Kind::Query) => Ok(statement),
        (AgentAccess::ReadOnly, kind) => Err(AgentError::Denied(format!(
            "`{connection}` is read-only for agents: only requests that change nothing may run, \
             and this one {}. The user can allow agents to write in PlatypusDB's connection \
             settings (Agent access), except on production and read-only connections",
            match kind {
                Kind::Dml => "changes documents",
                Kind::Ddl => "changes indices, mappings or aliases",
                Kind::Other | Kind::Query => "changes the server or is not known to only read",
            }
        ))),
        _ => match parsed.unfiltered() {
            Some(every) if unfiltered == UnfilteredRule::Refuse => {
                Err(AgentError::Denied(format!(
                    "{} on `{}` changes every document or index it reaches: PlatypusDB's Safety \
             settings ask before such changes, and agents cannot answer",
                    every.verb, every.index
                )))
            }
            _ => Ok(statement),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: Dialect = Dialect::Postgres;

    #[test]
    fn checks_cql_statements() {
        let ro = |sql| check_cql(sql, AgentAccess::ReadOnly, UnfilteredRule::Refuse, "c");
        let rw = |sql| check_cql(sql, AgentAccess::ReadWrite, UnfilteredRule::Refuse, "c");
        assert_eq!(
            ro(" SELECT * FROM shop.orders WHERE id = 'a;b'; ").unwrap(),
            "SELECT * FROM shop.orders WHERE id = 'a;b'"
        );
        assert!(matches!(
            ro("INSERT INTO t (id) VALUES (1)"),
            Err(AgentError::Denied(_))
        ));
        assert!(matches!(ro("TRUNCATE t"), Err(AgentError::Denied(_))));
        assert!(matches!(ro("DROP TABLE t"), Err(AgentError::Denied(_))));
        assert!(matches!(ro("USE other"), Err(AgentError::Denied(_))));
        assert!(matches!(
            ro("SELECT 1; SELECT 2"),
            Err(AgentError::Invalid(_))
        ));
        assert!(matches!(ro("  -- nothing\n"), Err(AgentError::Invalid(_))));
        // A batch hides its statements from the check, so it is refused.
        let batch =
            "BEGIN BATCH INSERT INTO t (id) VALUES (1); DELETE FROM t WHERE id = 2; APPLY BATCH";
        assert!(matches!(ro(batch), Err(AgentError::Denied(_))));
        assert!(matches!(rw(batch), Err(AgentError::Denied(_))));
        // Writes with a key may run; a TRUNCATE is what the Safety setting asks about.
        assert!(rw("UPDATE t SET v = 1 WHERE id = 2").is_ok());
        assert!(rw("DELETE FROM t WHERE id = 2").is_ok());
        assert!(rw("DROP TABLE t").is_ok());
        assert!(matches!(rw("TRUNCATE t"), Err(AgentError::Denied(_))));
        // A `//` comment is a comment in CQL, so it cannot hide the statement after it.
        assert!(matches!(rw("// x\nTRUNCATE t"), Err(AgentError::Denied(_))));
        assert!(matches!(ro("// x\nTRUNCATE t"), Err(AgentError::Denied(_))));
        assert_eq!(ro("// x\nSELECT 1").unwrap(), "// x\nSELECT 1");
        assert!(
            check_cql(
                "TRUNCATE t",
                AgentAccess::ReadWrite,
                UnfilteredRule::Allow,
                "c"
            )
            .is_ok()
        );
        assert!(matches!(
            check_cql("SELECT 1", AgentAccess::Off, UnfilteredRule::Allow, "c"),
            Err(AgentError::NotFound(_))
        ));
        assert!(changes(
            "INSERT INTO t (id) VALUES (1)",
            DriverKind::Cassandra
        ));
        assert!(!changes("SELECT * FROM t", DriverKind::Cassandra));
    }

    #[test]
    fn checks_mongosh_statements() {
        let ro = |sql| check_mongo(sql, AgentAccess::ReadOnly, UnfilteredRule::Refuse, "m");
        assert_eq!(
            ro(" db.orders.find({ a: 1 }); ").unwrap(),
            "db.orders.find({ a: 1 })"
        );
        assert!(denied(ro("db.orders.deleteOne({ a: 1 })")).contains("changes documents"));
        assert!(denied(ro("db.runCommand({ drop: 'orders' })")).contains("command"));
        assert!(denied(ro("use other")).contains("database"));
        assert!(matches!(
            ro("db.a.find()\ndb.b.find()"),
            Err(AgentError::Invalid(_))
        ));
        let rw = |sql| check_mongo(sql, AgentAccess::ReadWrite, UnfilteredRule::Refuse, "m");
        assert!(rw("db.orders.deleteMany({ a: 1 })").is_ok());
        assert!(denied(rw("db.orders.deleteMany({})")).contains("every document of `orders`"));
    }

    fn denied(r: Result<&str>) -> String {
        match r {
            Err(AgentError::Denied(m)) => m,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn checks_redis_commands() {
        let ro = |sql| check_redis(sql, AgentAccess::ReadOnly, UnfilteredRule::Refuse, "r");
        assert_eq!(ro(" GET a ").unwrap(), "GET a");
        assert_eq!(ro("BROWSE db1.hash").unwrap(), "BROWSE db1.hash");
        assert!(denied(ro("SET a 1")).contains("may run, and this one changes keys. The"));
        assert!(denied(ro("FLUSHDB")).contains("empties"));
        assert!(denied(ro("SELECT 2")).contains("session"));
        assert!(matches!(ro("GET a\nGET b"), Err(AgentError::Invalid(_))));
        for sql in [
            "CONFIG GET requirepass",
            "CONFIG GET *",
            "ACL LIST",
            "ACL GETUSER default",
        ] {
            assert!(denied(ro(sql)).contains("credentials"), "{sql}");
            // A read-write agent's user is asked first, as for a change.
            assert!(changes(sql, DriverKind::Redis), "{sql}");
        }
        assert!(ro("CONFIG GET maxmemory").is_ok());
        let rw = |sql| check_redis(sql, AgentAccess::ReadWrite, UnfilteredRule::Refuse, "r");
        assert!(rw("SET a 1").is_ok());
        assert!(denied(rw("FLUSHALL")).contains("changes, and agents cannot"));
        assert!(matches!(rw("SELECT 1"), Err(AgentError::Invalid(_))));
        assert!(changes("DEL a", DriverKind::Redis));
        assert!(!changes("HGETALL h", DriverKind::Redis));
    }

    #[test]
    fn checks_search_requests() {
        let ro = |sql| check_search(sql, AgentAccess::ReadOnly, UnfilteredRule::Refuse, "s");
        assert_eq!(ro(" GET /a/_search ").unwrap(), "GET /a/_search");
        assert_eq!(ro("SELECT 1").unwrap(), "SELECT 1");
        assert_eq!(
            ro("POST /a/_search\n{\"query\":{\"match_all\":{}}}").unwrap(),
            "POST /a/_search\n{\"query\":{\"match_all\":{}}}"
        );
        assert!(denied(ro("POST /a/_doc\n{}")).contains("changes documents"));
        assert!(denied(ro("DELETE /a")).contains("changes indices"));
        assert!(denied(ro("PUT /_cluster/settings\n{}")).contains("changes the server"));
        assert!(matches!(
            ro("GET /a/_search\n\nGET /b/_search"),
            Err(AgentError::Invalid(_))
        ));
        assert!(matches!(ro(""), Err(AgentError::Invalid(_))));
        // A search path that resolves to a write, or a URL to another host, never runs.
        for sneaky in [
            "POST /a/_search/../_bulk\n{\"delete\":{\"_id\":\"1\"}}",
            "POST /a/_search/%2e%2e/_bulk\n{\"delete\":{\"_id\":\"1\"}}",
            "GET http://attacker.example/x",
            "GET //attacker.example/x",
        ] {
            assert!(
                matches!(ro(sneaky), Err(AgentError::Invalid(_))),
                "{sneaky}"
            );
        }
        let rw = |sql| check_search(sql, AgentAccess::ReadWrite, UnfilteredRule::Refuse, "s");
        assert!(rw("POST /a/_doc\n{\"x\":1}").is_ok());
        assert!(rw("POST /a/_delete_by_query\n{\"query\":{\"term\":{\"x\":1}}}").is_ok());
        assert!(denied(rw("POST /a/_delete_by_query")).contains("cannot answer"));
        assert!(denied(rw("POST /a/_delete_by_query?q=%2A")).contains("cannot answer"));
        assert!(
            denied(rw("POST /a/_delete_by_query\n{\"query\":{\"bool\":{}}}"))
                .contains("cannot answer")
        );
        assert!(denied(rw("DELETE /_all")).contains("every document or index"));
        let allow = |sql| check_search(sql, AgentAccess::ReadWrite, UnfilteredRule::Allow, "s");
        assert!(allow("POST /a/_delete_by_query").is_ok());
        let off = check_search(
            "GET /a/_search",
            AgentAccess::Off,
            UnfilteredRule::Allow,
            "s",
        );
        assert!(matches!(off, Err(AgentError::NotFound(_))));
        assert!(changes("DELETE /a", DriverKind::Elasticsearch));
        assert!(!changes("GET /a/_search", DriverKind::Elasticsearch));
    }

    #[test]
    fn read_only_runs_queries_only() {
        let ro = |sql| check(sql, PG, AgentAccess::ReadOnly, UnfilteredRule::Allow, "db");
        assert_eq!(ro("  select * from t;  ").unwrap(), "select * from t");
        assert_eq!(ro("explain select 1").unwrap(), "explain select 1");
        assert!(denied(ro("update t set a = 1 where id = 2")).contains("changes data"));
        assert!(denied(ro("drop table t")).contains("schema"));
        assert!(denied(ro("set search_path = x")).contains("session"));
        assert!(
            denied(ro("with d as (delete from t returning *) select * from d")).contains("data")
        );
        assert!(denied(ro("select * into copy from t")).contains("data"));
        assert!(denied(ro("begin")).contains("session"));
        assert!(denied(ro("@set x = 1")).contains("client commands"));
    }

    #[test]
    fn read_only_refuses_effects_outside_the_transaction() {
        let ro = |sql, dialect| {
            check(
                sql,
                dialect,
                AgentAccess::ReadOnly,
                UnfilteredRule::Allow,
                "db",
            )
        };
        for sql in [
            "select pg_terminate_backend(42)",
            "select pg_reload_conf()",
            "SELECT pg_catalog.PG_NOTIFY('c', 'x')",
            "select * from dblink('host=x', 'select 1') as t(a int)",
            "select dblink_exec('drop table t')",
            "select pg_advisory_lock(1)",
            "select pg_try_advisory_lock_shared(1)",
            "select lo_export(1, '/tmp/x')",
            "select pg_file_write('x', 'y', false)",
            r#"select "pg_promote"()"#,
        ] {
            // pg_terminate_backend is caught by the analysis already.
            assert!(!denied(ro(sql, PG)).is_empty(), "{sql}");
        }
        assert!(denied(ro("select pg_reload_conf()", PG)).contains("outside the transaction"));
        let my = Dialect::MySql;
        assert!(denied(ro("SELECT LOAD_FILE('/etc/passwd')", my)).contains("`LOAD_FILE`"));
        assert!(ro("select * from t into outfile '/tmp/x'", my).is_err());
        assert!(ro("select * from t into dumpfile '/tmp/x'", my).is_err());
        // Names in strings are only text, and other databases have no such functions.
        assert!(ro("select 'pg_notify' as name", PG).is_ok());
        assert!(ro("select load_file from t", PG).is_ok());
        assert!(ro("select pg_notify from t", Dialect::Sqlite).is_ok());
    }

    #[test]
    fn read_only_refuses_snowflake_system_functions_and_calls() {
        let ro = |sql, dialect| {
            check(
                sql,
                dialect,
                AgentAccess::ReadOnly,
                UnfilteredRule::Allow,
                "db",
            )
        };
        let sf = Dialect::Snowflake;
        for sql in [
            "select system$cancel_query('01a')",
            "SELECT SYSTEM$ABORT_SESSION(42)",
            "select SYSTEM$CANCEL_ALL_QUERIES(42)",
            r#"select "SYSTEM$PIPE_FORCE_RESUME"('p')"#,
        ] {
            assert!(
                denied(ro(sql, sf)).contains("outside the transaction"),
                "{sql}"
            );
        }
        assert!(ro("select 'system$cancel_query' as name", sf).is_ok());
        // A backslash-escaped quote keeps the rest inside the string in Snowflake, so the
        // `DROP` after it is a second statement.
        assert!(matches!(
            ro(r"SELECT 'a\'' ; DROP TABLE t; --'", sf),
            Err(AgentError::Invalid(m)) if m.contains("one statement at a time")
        ));
        for dialect in [
            Dialect::Ansi,
            Dialect::Snowflake,
            Dialect::BigQuery,
            Dialect::Databricks,
        ] {
            assert!(
                denied(ro("call system.runtime.kill_query('q')", dialect)).contains("read-only"),
                "{dialect:?}"
            );
        }
    }

    /// Statements an agent with read-only access may try to write or reach outside with:
    /// each is refused, as a write, an outside effect or more than one statement.
    mod bypass {
        use platypus_sql::Dialect::{self, *};

        pub(super) const ATTEMPTS: &[(Dialect, &str)] = &[
            (MySql, "SELECT 1 /*! INTO OUTFILE '/tmp/x' */"),
            (MySql, "SELECT /*! LOAD_FILE('/etc/passwd') */"),
            (MySql, "SELECT /*M!100100 LOAD_FILE('/etc/passwd') */"),
            (MySql, "SELECT 1 /*!; COMMIT; DELETE FROM t */"),
            (
                MySql,
                "DELIMITER //\nSELECT 1; SET GLOBAL general_log = 1//",
            ),
            // Read as the server does under NO_BACKSLASH_ESCAPES.
            (MySql, r"SELECT 'a\' , LOAD_FILE('/etc/passwd') -- '"),
            (MySql, r"SELECT 'a\'; COMMIT; DELETE FROM t; -- '"),
            (MySql, "SELECT GET_LOCK('x', 10)"),
            (
                SqlServer,
                "SELECT 1 COMMIT EXEC('INS'+'ERT INTO t VALUES (1)')",
            ),
            (SqlServer, "SELECT 1 EXEC xp_cmdshell 'whoami'"),
            (SqlServer, "[xp_cmdshell] 'whoami'"),
            (SqlServer, "SELECT 0xINSERT INTO t VALUES (1)"),
            (
                SqlServer,
                "SELECT * FROM OPENROWSET(BULK 'c:\\secret.txt', SINGLE_CLOB) x",
            ),
            (SqlServer, "SELECT * FROM OPENQUERY(srv, 'DELETE FROM t')"),
            (
                Postgres,
                "select query_to_xml('select pg_reload_conf()', true, true, '')",
            ),
            (Postgres, "select pg_read_file('/etc/passwd')"),
            (Postgres, "select * from pg_ls_dir('.')"),
            (Postgres, "select pg_create_restore_point('x')"),
            // Names hidden behind PostgreSQL's Unicode escapes.
            (Postgres, r#"SELECT U&"\0070g_advisory_lock"(4243)"#),
            (Postgres, r#"SELECT u&"\0070g_read_file"('PG_VERSION')"#),
            (Postgres, r#"SELECT U&"!0070g_ls_dir" UESCAPE '!' ('.')"#),
            (
                Postgres,
                r#"SELECT U&"!0070g_ls_dir" /* c */ uescape '!' ('.')"#,
            ),
            (Postgres, r#"SELECT U&"\+000070g_terminate_backend"(1)"#),
            (
                Postgres,
                r#"SELECT pg_catalog.U&"lo\005fexport"(1, '/tmp/x')"#,
            ),
            (Postgres, r#"SELECT U&"\zzzz"(1)"#),
            (Postgres, "select pg_stat_reset()"),
            (Postgres, "select pg_stat_reset_shared('bgwriter')"),
            (Postgres, "select pg_stat_statements_reset()"),
            // And behind ClickHouse's and BigQuery's backslash escapes.
            (ClickHouse, r"SELECT * FROM `fi\x6ce`('x.csv')"),
            (ClickHouse, r#"SELECT * FROM "\x75rl"('http://x', CSV)"#),
            (BigQuery, r"SELECT * FROM `external\x5fquery`('c', 'x')"),
            (Postgres, "select 1 -- x\r; select pg_reload_conf()"),
            (Postgres, r"select 'x\''; delete from t; --'"),
            (Postgres, "copy (select 1) to program 'id'"),
            (DuckDb, "SELECT * FROM read_csv('/etc/passwd')"),
            (DuckDb, "select * from read_text('~/.ssh/id_rsa')"),
            (DuckDb, "SELECT * FROM 'C:/Users/x/.ssh/id_rsa'"),
            (DuckDb, "SELECT * FROM \"secrets.csv\""),
            (DuckDb, "SELECT * FROM t, 'x.parquet'"),
            (
                DuckDb,
                "SELECT * FROM t JOIN 'https://example.com/x.csv' USING (a)",
            ),
            (DuckDb, "SELECT * FROM query('SELECT 1')"),
            (DuckDb, "SELECT getenv('HOME')"),
            (DuckDb, "SELECT * FROM glob('*')"),
            (DuckDb, "select * from sqlite_scan('x.db', 't')"),
            (DuckDb, "COPY t TO 'x.csv'"),
            (DuckDb, "INSTALL httpfs"),
            (DuckDb, "ATTACH 'x.db'"),
            (Sqlite, "select load_extension('x')"),
            (Sqlite, "select writefile('x', 'y')"),
            (Sqlite, "ATTACH 'x.db' AS y"),
            (Sqlite, "PRAGMA writable_schema = 1"),
            (ClickHouse, "SELECT * FROM file('x.csv')"),
            (
                ClickHouse,
                "SELECT * FROM url('http://169.254.169.254/', CSV)",
            ),
            (
                ClickHouse,
                "SELECT * FROM executable('x.sh', TSV, 'a String')",
            ),
            (ClickHouse, "SELECT * FROM remote('h', db.t)"),
            (
                Oracle,
                "select dbms_xmlquery.newcontext('begin null; end;') from dual",
            ),
            (Oracle, "select utl_http.request('http://x') from dual"),
            (Oracle, "select httpuritype('http://x').getclob() from dual"),
            (
                Oracle,
                "WITH FUNCTION f RETURN NUMBER IS BEGIN RETURN 1; END; SELECT f FROM dual",
            ),
            (
                Ansi,
                "SELECT * FROM TABLE(system.query(query => 'DELETE FROM t'))",
            ),
            (
                Databricks,
                "SELECT reflect('java.lang.Runtime', 'getRuntime')",
            ),
            (
                BigQuery,
                "SELECT * FROM EXTERNAL_QUERY('c', 'DELETE FROM t')",
            ),
        ];
    }

    #[test]
    fn read_only_refuses_bypass_attempts() {
        use Dialect::*;
        let attempts = bypass::ATTEMPTS;
        for &(dialect, sql) in attempts {
            let checked = check(
                sql,
                dialect,
                AgentAccess::ReadOnly,
                UnfilteredRule::Allow,
                "db",
            );
            assert!(
                matches!(checked, Err(AgentError::Denied(_) | AgentError::Invalid(_))),
                "{dialect:?}: {sql}: {checked:?}"
            );
        }
        let queries: &[(Dialect, &str)] = &[
            (MySql, "SELECT /*+ BKA(t) */ * FROM t"),
            (MySql, "SELECT /*! STRAIGHT_JOIN */ a FROM t"),
            (
                SqlServer,
                "SELECT a FROM t OPTION (USE HINT ('DISABLE_OPTIMIZER_ROWGOAL'))",
            ),
            (ClickHouse, "SELECT url, file FROM t"),
            (DuckDb, "SELECT query FROM t WHERE a = 'x.csv'"),
            (DuckDb, "SELECT 'x.csv' AS f, read FROM t"),
            (Oracle, "select dbms_lob.substr(c, 10, 1) from t"),
            (Postgres, r#"SELECT U&"\0061" FROM t"#),
            (Ansi, "SELECT query FROM system.runtime.queries"),
        ];
        for &(dialect, sql) in queries {
            let checked = check(
                sql,
                dialect,
                AgentAccess::ReadOnly,
                UnfilteredRule::Allow,
                "db",
            );
            assert!(checked.is_ok(), "{dialect:?}: {sql}: {checked:?}");
        }
    }

    /// ClickHouse table functions are allowed by name: a new one that reaches out is refused.
    #[test]
    fn read_only_allows_local_clickhouse_table_functions_only() {
        let ro = |sql| {
            check(
                sql,
                Dialect::ClickHouse,
                AgentAccess::ReadOnly,
                UnfilteredRule::Allow,
                "db",
            )
        };
        for sql in [
            "SELECT * FROM deltaLake('http://h:1/b/', 'k', 's')",
            "SELECT * FROM iceberg('http://h:1/b/', 'k', 's')",
            "SELECT * FROM icebergS3('http://h:1/b/')",
            "SELECT * FROM hudi('http://h:1/b/', 'k', 's')",
            "SELECT * FROM fileCluster('default', 'x.csv')",
            "SELECT * FROM ytsaurus('http://h', '//t', 'tok', 'a String')",
            "SELECT * FROM paimon('http://h:1/b/')",
            "SELECT * FROM azureBlobStorageCluster('c', 'u', 'x', 'y')",
            "SELECT * FROM t, someFutureFunction('x')",
            "SELECT * FROM t JOIN `deltaLake`('http://h:1/b/') AS d ON 1",
            "SELECT * FROM (SELECT * FROM hudiCluster('c', 'http://h/b/'))",
            "SELECT * FROM view(SELECT * FROM deltaLake('http://h/b/'))",
            "SELECT 1 WHERE 1 IN (SELECT 1 FROM iceberg('http://h/b/'))",
            "DESCRIBE TABLE deltaLake('http://h/b/')",
            "DESC iceberg('http://h/b/')",
        ] {
            let message = denied(ro(sql));
            assert!(
                message.contains("outside the transaction"),
                "{sql}: {message}"
            );
        }
        for sql in [
            "SELECT * FROM numbers(10)",
            "SELECT * FROM system.numbers LIMIT 1",
            "SELECT * FROM t, generateRandom('a Int') LIMIT 1",
            "SELECT * FROM values('a Int', 1)",
            "SELECT * FROM view(SELECT 1)",
            "SELECT * FROM dictionary('d')",
            "SELECT extract(DAY FROM now()), trim(BOTH ' ' FROM toString(1))",
            "SELECT a FROM t ARRAY JOIN arrayMap(x -> x, [1]) AS a",
            "SELECT * FROM t JOIN u ON t.id = u.id WHERE f(1) > 0",
            "SELECT count() FROM (SELECT 1)",
            "SELECT * FROM t WHERE a IS NOT DISTINCT FROM lower(b)",
            "SELECT * FROM t WHERE (a, b) IS DISTINCT FROM lower(c)",
            "SELECT a IS DISTINCT FROM upper(b) FROM t, u",
        ] {
            assert!(ro(sql).is_ok(), "{sql}: {:?}", ro(sql));
        }
        // A table after a comparison is still one.
        let message = denied(ro(
            "SELECT a IS DISTINCT FROM b FROM t, deltaLake('http://h/b/')",
        ));
        assert!(message.contains("outside the transaction"), "{message}");
    }

    #[test]
    fn one_statement_per_call() {
        let rw = |sql| check(sql, PG, AgentAccess::ReadWrite, UnfilteredRule::Allow, "db");
        assert!(
            matches!(rw("select 1; select 2"), Err(AgentError::Invalid(m)) if m.contains("has 2"))
        );
        assert!(matches!(rw(" -- nothing\n"), Err(AgentError::Invalid(_))));
    }

    #[test]
    fn read_write_keeps_the_safety_checks() {
        let rw = |sql, rule| check(sql, PG, AgentAccess::ReadWrite, rule, "db");
        assert!(rw("update t set a = 1 where id = 2", UnfilteredRule::Refuse).is_ok());
        assert!(rw("create table x (id int)", UnfilteredRule::Refuse).is_ok());
        let every = denied(rw("delete from app.t", UnfilteredRule::Refuse));
        assert!(every.contains("every row of `app.t`"), "{every}");
        assert!(denied(rw("truncate t", UnfilteredRule::Refuse)).contains("`t`"));
        assert!(rw("delete from t", UnfilteredRule::Allow).is_ok());
        assert!(denied(rw("commit", UnfilteredRule::Allow)).contains("commits on its own"));
        assert!(denied(rw("start transaction", UnfilteredRule::Allow)).contains("BEGIN"));
        let both = denied(rw("truncate a, s.b", UnfilteredRule::Refuse));
        assert!(both.contains("`a`, `s.b`"), "{both}");
        // A data-modifying common table expression behind a SELECT.
        let cte = denied(rw(
            "with d as (delete from t returning *) select count(*) from d",
            UnfilteredRule::Refuse,
        ));
        assert!(cte.contains("every row of `t`"), "{cte}");
    }

    /// A Transact-SQL batch needs no `;`, and a MySQL `DELIMITER` joins statements: the
    /// checks look at every statement in them.
    #[test]
    fn read_write_checks_every_statement_of_a_batch() {
        let rw = |sql, dialect| {
            check(
                sql,
                dialect,
                AgentAccess::ReadWrite,
                UnfilteredRule::Refuse,
                "db",
            )
        };
        let ms = Dialect::SqlServer;
        let every = denied(rw("UPDATE t SET a = 1 WHERE id = 1 DELETE FROM u", ms));
        assert!(every.contains("every row of `u`"), "{every}");
        let top = denied(rw("DELETE TOP (2) FROM qa_safe_g", ms));
        assert!(top.contains("`qa_safe_g`"), "{top}");
        let alias = denied(rw(
            "UPDATE g SET v = 1 FROM qa_safe_g g JOIN h ON 1 = 1",
            ms,
        ));
        assert!(alias.contains("`qa_safe_g`"), "{alias}");
        for sql in [
            "SELECT 1 BEGIN TRAN",
            "UPDATE t SET a = 1 WHERE id = 1 COMMIT",
        ] {
            assert!(denied(rw(sql, ms)).contains("BEGIN, COMMIT"), "{sql}");
        }
        assert!(rw("UPDATE t SET a = 1 WHERE id = 1", ms).is_ok());
        let my = Dialect::MySql;
        let joined = denied(rw(
            "DELIMITER //\nUPDATE t SET a = 1 WHERE id = 1; DELETE FROM u//",
            my,
        ));
        assert!(joined.contains("every row of `u`"), "{joined}");
        assert!(denied(rw("DELIMITER //\nSELECT 1; COMMIT//", my)).contains("BEGIN, COMMIT"));
        assert!(
            rw(
                "DELIMITER //\nCREATE PROCEDURE p() BEGIN DELETE FROM u; END//",
                my
            )
            .is_ok()
        );
    }

    #[test]
    fn off_hides_the_connection() {
        let off = check(
            "select 1",
            PG,
            AgentAccess::Off,
            UnfilteredRule::Allow,
            "db",
        );
        assert!(matches!(off, Err(AgentError::NotFound(_))));
    }

    #[test]
    fn reads_the_unfiltered_setting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        assert_eq!(UnfilteredRule::from_settings(None), UnfilteredRule::Refuse);
        let mut settings = SettingsStore::open(&path).unwrap();
        assert_eq!(
            UnfilteredRule::from_settings(Some(&settings)),
            UnfilteredRule::Refuse
        );
        settings
            .set("safety.unfiltered", Some("off".into()))
            .unwrap();
        assert_eq!(
            UnfilteredRule::from_settings(Some(&settings)),
            UnfilteredRule::Allow
        );
    }
}
