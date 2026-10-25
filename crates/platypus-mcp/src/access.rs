//! What agents may run: the connection's agent access, then the statement itself, checked
//! with the same analysis the app runs before a statement (`platypus_sql::analyze`).

use platypus_core::{TxEffect, classify};
use platypus_sql::{Dialect, Lexer, StatementKind, TokenKind};
pub use platypus_store::AgentAccess;
use platypus_store::SettingsStore;

use crate::error::{AgentError, Result};

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

/// PostgreSQL functions that act outside the transaction (other sessions, the server's
/// configuration and files, notifications, session locks): a `READ ONLY` transaction lets
/// them run.
const POSTGRES_OUTSIDE: &[&str] = &[
    "pg_terminate_backend",
    "pg_cancel_backend",
    "pg_reload_conf",
    "pg_rotate_logfile",
    "pg_promote",
    "lo_import",
    "lo_export",
    "pg_notify",
];
/// Families of them: `dblink_exec`, `pg_advisory_lock`, adminpack's `pg_file_write`…
const POSTGRES_OUTSIDE_PREFIXES: &[&str] =
    &["dblink", "pg_advisory_", "pg_try_advisory_", "pg_file_"];
/// MySQL and MariaDB read and write the server's files whatever the transaction.
const MYSQL_OUTSIDE: &[&str] = &["load_file", "outfile", "dumpfile"];

/// The first word of `statement` naming something that reaches outside a read-only
/// transaction in `dialect`, as written.
fn outside_effect(statement: &str, dialect: Dialect) -> Option<&str> {
    let (names, prefixes): (&[&str], &[&str]) = match dialect {
        Dialect::Postgres => (POSTGRES_OUTSIDE, POSTGRES_OUTSIDE_PREFIXES),
        Dialect::MySql => (MYSQL_OUTSIDE, &[]),
        Dialect::Sqlite | Dialect::DuckDb | Dialect::SqlServer | Dialect::ClickHouse => {
            return None;
        }
    };
    Lexer::new(statement, dialect)
        .filter_map(|t| match t.kind {
            TokenKind::Word => Some(t.text(statement)),
            TokenKind::QuotedIdent => Some(t.text(statement).trim_matches(['"', '`', '[', ']'])),
            _ => None,
        })
        .find(|word| {
            let word = word.to_ascii_lowercase();
            names.contains(&word.as_str()) || prefixes.iter().any(|p| word.starts_with(p))
        })
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
                "run_query runs one statement at a time and this SQL has {}: send them in separate calls",
                many.len()
            )));
        }
    };
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
        (AgentAccess::ReadWrite, _)
            if matches!(classify(statement), TxEffect::Begin | TxEffect::End) =>
        {
            Err(AgentError::Denied(
                "each statement an agent runs commits on its own: BEGIN, COMMIT and ROLLBACK \
                 are not available"
                    .to_owned(),
            ))
        }
        (AgentAccess::ReadWrite, _) => match (&analysis.unfiltered, unfiltered) {
            (Some(table), UnfilteredRule::Refuse) => Err(AgentError::Denied(format!(
                "this statement changes every row of `{}`: add a WHERE. PlatypusDB's Safety \
                 settings ask before UPDATE or DELETE without WHERE and TRUNCATE, and agents \
                 cannot answer",
                table.schema.as_ref().map_or_else(
                    || table.name.clone(),
                    |schema| format!("{schema}.{}", table.name)
                )
            ))),
            _ => Ok(statement),
        },
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
                "run_query runs one statement at a time and this script has {}: send them in separate calls",
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

#[cfg(test)]
mod tests {
    use super::*;

    const PG: Dialect = Dialect::Postgres;

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
