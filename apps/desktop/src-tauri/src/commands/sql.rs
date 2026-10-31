//! SQL text analysis for the editor.

use platypus_core::DriverKind;
use platypus_sql::{
    Dialect, FilterColumn, FormatOptions, KeywordCase, LintKind, ParameterKind, SelectItem,
    StatementKind,
};
use serde::{Deserialize, Serialize};

/// A table a statement changes in every row; `schema` is `None` for the session's.
#[derive(Debug, Serialize)]
pub struct UnfilteredTable {
    schema: Option<String>,
    name: String,
    /// What changes it: `UPDATE`, `DELETE`, `TRUNCATE`, or mongosh's `updateMany`, `deleteMany`.
    verb: &'static str,
    /// Transact-SQL's `TOP (n)`: up to that many rows the database picks, not every one.
    limited: bool,
}

/// A statement range in UTF-16 code units, the unit JavaScript strings use, and what the
/// statement does, for the checks before it runs.
#[derive(Debug, Serialize)]
pub struct StatementSpan {
    start: usize,
    end: usize,
    read_only: bool,
    /// `query`, `dml`, `ddl`, `other` or `command` (`@set`, run by the app).
    kind: &'static str,
    /// The tables an `UPDATE` or `DELETE` without `WHERE`, or a `TRUNCATE`, changes in every
    /// row: several for `TRUNCATE a, b` or a Transact-SQL batch.
    unfiltered: Vec<UnfilteredTable>,
    /// Its `:name`, `?` and `${name}` placeholders, in the same units as `start`.
    parameters: Vec<ParameterSpan>,
}

#[derive(Debug, Serialize)]
pub struct ParameterSpan {
    /// `named`, `positional` or `variable`.
    kind: &'static str,
    name: String,
    start: usize,
    end: usize,
}

/// Statements a script sends together: a Transact-SQL batch up to `GO`.
#[derive(Debug, Serialize)]
pub struct BatchSpan {
    /// Indices into `statements`, `first..end`.
    first: usize,
    end: usize,
    /// How many times it runs (`GO 3`).
    repeat: u32,
}

#[derive(Debug, Serialize)]
pub struct SqlAnalysis {
    statements: Vec<StatementSpan>,
    /// Index into `statements` of the one to run for the cursor.
    at_cursor: Option<usize>,
    /// Groups of statements that a script runs together; empty when each runs alone.
    batches: Vec<BatchSpan>,
}

/// The end of statement `s` as it is sent: in Transact-SQL with its `;`, which `MERGE`
/// requires (and every statement accepts); a client command (`@set`) never takes it.
fn sent_end(sql: &str, dialect: Dialect, s: platypus_sql::Statement) -> usize {
    if dialect == Dialect::SqlServer
        && sql[s.end..].starts_with(';')
        && !s.text(sql).starts_with('@')
    {
        s.end + 1
    } else {
        s.end
    }
}

/// Maps byte offsets of `src` to UTF-16 offsets and back.
struct Utf16Map {
    /// `(byte offset, utf16 offset)` at every char boundary, plus the end.
    points: Vec<(usize, usize)>,
}

impl Utf16Map {
    fn new(src: &str) -> Self {
        let mut points = Vec::with_capacity(src.len() + 1);
        let mut utf16 = 0;
        for (byte, c) in src.char_indices() {
            points.push((byte, utf16));
            utf16 += c.len_utf16();
        }
        points.push((src.len(), utf16));
        Self { points }
    }

    fn to_utf16(&self, byte: usize) -> usize {
        let i = self.points.partition_point(|&(b, _)| b < byte);
        self.points.get(i).map_or(0, |&(_, u)| u)
    }

    fn to_byte(&self, utf16: usize) -> usize {
        let i = self.points.partition_point(|&(_, u)| u < utf16);
        self.points
            .get(i)
            .map_or_else(|| self.points.last().map_or(0, |p| p.0), |&(b, _)| b)
    }
}

/// Splits `sql` into statements and finds the one under `cursor` (UTF-16 offset).
#[tauri::command]
pub fn analyze_sql(sql: String, driver: DriverKind, cursor: usize) -> SqlAnalysis {
    let map = Utf16Map::new(&sql);
    if driver == DriverKind::Mongo {
        return analyze_mongo(&sql, &map, cursor);
    }
    if driver == DriverKind::Redis {
        return analyze_redis(&sql, &map, cursor);
    }
    if driver == DriverKind::Elasticsearch {
        return analyze_search(&sql, &map, cursor);
    }
    if driver == DriverKind::Cassandra {
        return analyze_cql(&sql, &map, cursor);
    }
    let dialect = Dialect::from(driver);
    let statements = platypus_sql::split(&sql, dialect);
    let at = platypus_sql::statement_at(&sql, dialect, map.to_byte(cursor));
    let batches = if dialect == Dialect::SqlServer {
        platypus_sql::batches(&sql, dialect)
            .into_iter()
            .map(|b| BatchSpan {
                first: b.statements.start,
                end: b.statements.end,
                repeat: b.repeat,
            })
            .collect()
    } else {
        Vec::new()
    };
    SqlAnalysis {
        at_cursor: at.and_then(|at| statements.iter().position(|s| *s == at)),
        batches,
        statements: statements
            .iter()
            .map(|s| {
                let text = s.text(&sql);
                let analysis = platypus_sql::analyze(text, dialect);
                let parameters = if analysis.kind == StatementKind::Command {
                    Vec::new()
                } else {
                    platypus_sql::parameters(text, dialect)
                };
                StatementSpan {
                    start: map.to_utf16(s.start),
                    end: map.to_utf16(sent_end(&sql, dialect, *s)),
                    read_only: matches!(
                        analysis.kind,
                        StatementKind::Query | StatementKind::Command
                    ),
                    kind: match analysis.kind {
                        StatementKind::Query => "query",
                        StatementKind::Dml => "dml",
                        StatementKind::Ddl => "ddl",
                        StatementKind::Other => "other",
                        StatementKind::Command => "command",
                    },
                    unfiltered: analysis
                        .unfiltered
                        .into_iter()
                        .map(|u| UnfilteredTable {
                            schema: u.table.schema,
                            name: u.table.name,
                            verb: u.verb,
                            limited: u.limited,
                        })
                        .collect(),
                    parameters: parameters
                        .into_iter()
                        .map(|p| ParameterSpan {
                            kind: match p.kind {
                                ParameterKind::Named => "named",
                                ParameterKind::Positional => "positional",
                                ParameterKind::Variable => "variable",
                            },
                            name: p.name,
                            start: map.to_utf16(s.start + p.start),
                            end: map.to_utf16(s.start + p.end),
                        })
                        .collect(),
                }
            })
            .collect(),
    }
}

/// Whether every statement of `sql` only reads, by the analysis the editor shows: what a
/// read-only connection lets run.
pub(crate) fn only_reads(sql: &str, driver: DriverKind) -> bool {
    use platypus_driver_mongo::shell::{self, StatementKind as Kind};
    if driver == DriverKind::Mongo {
        return shell::split(sql).iter().all(|range| {
            shell::parse(&sql[range.clone()]).is_ok_and(|c| shell::kind(&c) == Kind::Query)
        });
    }
    if driver == DriverKind::Redis {
        use platypus_driver_redis::command;
        return command::split(sql).iter().all(|range| {
            command::parse(&sql[range.clone()]).is_ok_and(|c| c.kind() == command::Kind::Query)
        });
    }
    if driver == DriverKind::Elasticsearch {
        return platypus_driver_search::command::only_reads(sql);
    }
    if driver == DriverKind::Cassandra {
        // A batch is no read, though the analysis sees `BEGIN`: it holds writes.
        return platypus_driver_cassandra::cql::split(sql)
            .iter()
            .all(|range| {
                let text = &sql[range.clone()];
                !is_cql_batch(text)
                    && platypus_sql::analyze(
                        &platypus_driver_cassandra::cql::for_analysis(text),
                        Dialect::Ansi,
                    )
                    .kind
                        == StatementKind::Query
            });
    }
    let dialect = Dialect::from(driver);
    platypus_sql::split(sql, dialect)
        .iter()
        .all(|s| platypus_sql::analyze(s.text(sql), dialect).kind == StatementKind::Query)
}

/// [`analyze_sql`] for mongosh statements, which have no parameters.
fn analyze_mongo(src: &str, map: &Utf16Map, cursor: usize) -> SqlAnalysis {
    use platypus_driver_mongo::shell::{self, CollectionOp, Command, StatementKind as Kind};
    let statements = shell::split(src);
    let at = shell::statement_at(src, map.to_byte(cursor));
    SqlAnalysis {
        at_cursor: at.and_then(|at| statements.iter().position(|s| *s == at)),
        batches: Vec::new(),
        statements: statements
            .iter()
            .map(|range| {
                let parsed = shell::parse(&src[range.clone()]).ok();
                let kind = parsed.as_ref().map_or(Kind::Other, shell::kind);
                StatementSpan {
                    start: map.to_utf16(range.start),
                    end: map.to_utf16(range.end),
                    read_only: kind == Kind::Query,
                    kind: match kind {
                        Kind::Query => "query",
                        Kind::Dml => "dml",
                        Kind::Ddl => "ddl",
                        Kind::Other => "other",
                    },
                    unfiltered: parsed
                        .as_ref()
                        .and_then(|command| {
                            let (schema, name) = shell::unfiltered(command)?;
                            let verb = match command {
                                Command::Collection {
                                    op: CollectionOp::Delete { .. },
                                    ..
                                } => "deleteMany",
                                _ => "updateMany",
                            };
                            Some(UnfilteredTable {
                                schema: schema.map(str::to_owned),
                                name: name.to_owned(),
                                verb,
                                limited: false,
                            })
                        })
                        .into_iter()
                        .collect(),
                    parameters: Vec::new(),
                }
            })
            .collect(),
    }
}

/// [`analyze_sql`] for Redis commands, one per line, which have no parameters.
fn analyze_redis(src: &str, map: &Utf16Map, cursor: usize) -> SqlAnalysis {
    use platypus_driver_redis::command::{self, Command, Kind};
    let flush = |verb: &'static str| UnfilteredTable {
        schema: None,
        name: String::new(),
        verb,
        limited: false,
    };
    let statements = command::split(src);
    let at = command::statement_at(src, map.to_byte(cursor));
    SqlAnalysis {
        at_cursor: at.and_then(|at| statements.iter().position(|s| *s == at)),
        batches: Vec::new(),
        statements: statements
            .iter()
            .map(|range| {
                let parsed = command::parse(&src[range.clone()]);
                let kind = parsed.as_ref().map_or(Kind::Other, Command::kind);
                // `FLUSHDB` and `FLUSHALL` delete every key, as a DELETE without WHERE every
                // row: the same Safety setting asks first.
                let unfiltered = match parsed.as_ref().map(Command::name).as_deref() {
                    Ok("FLUSHDB") => vec![flush("FLUSHDB")],
                    Ok("FLUSHALL") => vec![flush("FLUSHALL")],
                    _ => Vec::new(),
                };
                StatementSpan {
                    start: map.to_utf16(range.start),
                    end: map.to_utf16(range.end),
                    read_only: kind == Kind::Query,
                    kind: match kind {
                        Kind::Query => "query",
                        Kind::Dml => "dml",
                        // Server changes (`CONFIG SET`, `SHUTDOWN`) ask on production as DDL.
                        Kind::Ddl | Kind::Other => "ddl",
                    },
                    unfiltered,
                    parameters: Vec::new(),
                }
            })
            .collect(),
    }
}

/// [`analyze_sql`] for Cassandra and `ScyllaDB`: the statements are split as CQL splits them (a
/// batch is one statement), and each is read with the standard SQL analysis, which finds
/// `TRUNCATE` and the writes that name no row.
fn analyze_cql(src: &str, map: &Utf16Map, cursor: usize) -> SqlAnalysis {
    let ranges = platypus_driver_cassandra::cql::split(src);
    let byte = map.to_byte(cursor);
    // The statement under the cursor, else the last one before it, else the first.
    let at_cursor = ranges
        .iter()
        .position(|r| r.start <= byte && byte <= r.end)
        .or_else(|| ranges.iter().rposition(|r| r.end < byte))
        .or_else(|| (!ranges.is_empty()).then_some(0));
    SqlAnalysis {
        at_cursor,
        batches: Vec::new(),
        statements: ranges
            .iter()
            .map(|range| {
                let text = &src[range.clone()];
                // The SQL lexer reads CQL's `//` comments and `$$` strings from this rewrite.
                let readable = platypus_driver_cassandra::cql::for_analysis(text);
                let analysis = platypus_sql::analyze(&readable, Dialect::Ansi);
                // A batch hides its statements from the analysis: it asks as DDL does.
                let batch = is_cql_batch(text);
                StatementSpan {
                    start: map.to_utf16(range.start),
                    end: map.to_utf16(range.end),
                    read_only: !batch && analysis.kind == StatementKind::Query,
                    kind: match analysis.kind {
                        _ if batch => "ddl",
                        StatementKind::Query => "query",
                        StatementKind::Dml => "dml",
                        StatementKind::Ddl => "ddl",
                        StatementKind::Other => "other",
                        StatementKind::Command => "command",
                    },
                    unfiltered: analysis
                        .unfiltered
                        .into_iter()
                        .map(|u| UnfilteredTable {
                            schema: u.table.schema,
                            name: u.table.name,
                            verb: u.verb,
                            limited: u.limited,
                        })
                        .collect(),
                    parameters: parameter_spans(&readable, range.start, map),
                }
            })
            .collect(),
    }
}

/// Whether the CQL statement `text` is a `BEGIN … BATCH`, which the analysis cannot see into.
fn is_cql_batch(text: &str) -> bool {
    platypus_driver_cassandra::cql::first_word(text) == "BEGIN"
}

/// The placeholders of the statement `text`, which starts at byte `offset` of the editor's
/// text.
fn parameter_spans(text: &str, offset: usize, map: &Utf16Map) -> Vec<ParameterSpan> {
    platypus_sql::parameters(text, Dialect::Ansi)
        .into_iter()
        .map(|p| ParameterSpan {
            kind: match p.kind {
                ParameterKind::Named => "named",
                ParameterKind::Positional => "positional",
                ParameterKind::Variable => "variable",
            },
            name: p.name,
            start: map.to_utf16(offset + p.start),
            end: map.to_utf16(offset + p.end),
        })
        .collect()
}

/// [`analyze_sql`] for Elasticsearch: console requests and SQL statements, which have no
/// parameters.
fn analyze_search(src: &str, map: &Utf16Map, cursor: usize) -> SqlAnalysis {
    use platypus_driver_search::command::{self, Kind};
    let statements = command::split(src);
    let at = command::statement_at(src, map.to_byte(cursor));
    SqlAnalysis {
        at_cursor: at.and_then(|at| statements.iter().position(|s| *s == at)),
        batches: Vec::new(),
        statements: statements
            .iter()
            .map(|range| {
                let parsed = command::parse(&src[range.clone()]);
                let kind = parsed.as_ref().map_or(Kind::Other, command::Command::kind);
                // A `_delete_by_query` with no query, or the deletion of every index, asks
                // as a `DELETE` without `WHERE` does.
                let unfiltered = parsed
                    .ok()
                    .and_then(|c| c.unfiltered())
                    .map(|u| UnfilteredTable {
                        schema: None,
                        name: u.index,
                        verb: u.verb,
                        limited: false,
                    })
                    .into_iter()
                    .collect();
                StatementSpan {
                    start: map.to_utf16(range.start),
                    end: map.to_utf16(range.end),
                    read_only: kind == Kind::Query,
                    kind: match kind {
                        Kind::Query => "query",
                        Kind::Dml => "dml",
                        // Changes to indices and to the server ask on production as DDL.
                        Kind::Ddl | Kind::Other => "ddl",
                    },
                    unfiltered,
                    parameters: Vec::new(),
                }
            })
            .collect(),
    }
}

/// Whether the editor's text is SQL: not mongosh, Redis or Elasticsearch requests (which
/// take SQL among them, but not as the linter and the formatter read it).
const fn speaks_sql(driver: DriverKind) -> bool {
    !matches!(
        driver,
        DriverKind::Mongo | DriverKind::Redis | DriverKind::Elasticsearch
    )
}

/// A problem the linter found by itself; offsets in UTF-16 units of the whole text.
#[derive(Debug, Serialize)]
pub struct LintIssueSpan {
    /// `select_star` or `join_without_condition`.
    kind: &'static str,
    from: usize,
    to: usize,
}

#[derive(Debug, Serialize)]
pub struct LintTable {
    alias: String,
    /// `None` (with `name` too) for subqueries, functions and common table expressions.
    schema: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LintColumn {
    qualifier: String,
    column: String,
    from: usize,
    to: usize,
}

/// One statement's tables and qualified column references, which the editor checks
/// against its catalog.
#[derive(Debug, Serialize)]
pub struct LintStatement {
    tables: Vec<LintTable>,
    columns: Vec<LintColumn>,
}

#[derive(Debug, Serialize)]
pub struct SqlLint {
    issues: Vec<LintIssueSpan>,
    statements: Vec<LintStatement>,
}

/// Lints every statement of `sql` but the client commands.
#[tauri::command]
pub fn lint_sql(sql: String, driver: DriverKind) -> SqlLint {
    let dialect = Dialect::from(driver);
    let map = Utf16Map::new(&sql);
    let mut out = SqlLint {
        issues: Vec::new(),
        statements: Vec::new(),
    };
    // The linter checks SQL; mongosh and Redis commands have none of its issues.
    if !speaks_sql(driver) {
        return out;
    }
    for s in platypus_sql::split(&sql, dialect) {
        let text = s.text(&sql);
        if text.starts_with('@') {
            continue;
        }
        let lint = platypus_sql::lint(text, dialect);
        let span =
            |start: usize, end: usize| (map.to_utf16(s.start + start), map.to_utf16(s.start + end));
        out.issues.extend(lint.issues.iter().map(|i| {
            let (from, to) = span(i.start, i.end);
            LintIssueSpan {
                kind: match i.kind {
                    LintKind::SelectStar => "select_star",
                    LintKind::JoinWithoutCondition => "join_without_condition",
                },
                from,
                to,
            }
        }));
        out.statements.push(LintStatement {
            tables: lint
                .tables
                .into_iter()
                .map(|t| LintTable {
                    alias: t.alias,
                    schema: t.table.as_ref().and_then(|t| t.schema.clone()),
                    name: t.table.map(|t| t.name),
                })
                .collect(),
            columns: lint
                .columns
                .into_iter()
                .map(|c| {
                    let (from, to) = span(c.start, c.end);
                    LintColumn {
                        qualifier: c.qualifier,
                        column: c.column,
                        from,
                        to,
                    }
                })
                .collect(),
        });
    }
    out
}

/// The formatter settings, as the editor sends them.
#[derive(Debug, Deserialize)]
pub struct FormatSettings {
    /// `upper`, `lower` or `preserve`.
    keyword_case: String,
    indent: usize,
}

/// Formats `sql` for the editor; only whitespace and the case of keywords change.
#[tauri::command]
pub fn format_sql(sql: String, driver: DriverKind, settings: FormatSettings) -> String {
    if !speaks_sql(driver) {
        return sql;
    }
    let keyword_case = match settings.keyword_case.as_str() {
        "lower" => KeywordCase::Lower,
        "preserve" => KeywordCase::Preserve,
        _ => KeywordCase::Upper,
    };
    platypus_sql::format(
        &sql,
        Dialect::from(driver),
        FormatOptions {
            keyword_case,
            indent: settings.indent.clamp(1, 8),
        },
    )
}

/// An item of a query's select list, see [`SelectItem`].
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceItem {
    All,
    Column { name: String },
    Other,
}

/// The one table a query reads, so its results can be edited.
#[derive(Debug, Serialize)]
pub struct QuerySource {
    schema: Option<String>,
    name: String,
    items: Vec<SourceItem>,
}

/// The table `sql` reads when its rows are that table's rows; `None` for joins, grouping
/// and anything but a plain `SELECT`.
#[tauri::command]
pub fn query_source(sql: String, driver: DriverKind) -> Option<QuerySource> {
    if driver == DriverKind::Redis {
        use platypus_driver_redis::command::{self, Command};
        // `BROWSE` reads the rows of a key type's virtual table.
        let Ok(Command::Browse(browse)) = command::parse(sql.trim()) else {
            return None;
        };
        return Some(QuerySource {
            schema: browse.db.map(platypus_driver_redis::meta::schema_name),
            name: browse.kind.table().to_owned(),
            items: vec![SourceItem::All],
        });
    }
    if driver == DriverKind::Elasticsearch {
        // The search `BROWSE` writes: the documents of one index, whose changes the grid saves.
        let name = platypus_driver_search::command::source_index(sql.trim())?;
        return Some(QuerySource {
            schema: None,
            name,
            items: vec![SourceItem::All],
        });
    }
    if driver == DriverKind::Mongo {
        return None;
    }
    let source = platypus_sql::source_table(&sql, Dialect::from(driver))?;
    Some(QuerySource {
        schema: source.schema,
        name: source.name,
        items: source
            .items
            .into_iter()
            .map(|item| match item {
                SelectItem::All => SourceItem::All,
                SelectItem::Column(name) => SourceItem::Column { name },
                SelectItem::Other => SourceItem::Other,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_utf16_offsets() {
        // "é" is 2 bytes / 1 unit, "🦆" is 4 bytes / 2 units.
        let map = Utf16Map::new("aé🦆b");
        assert_eq!(map.to_utf16(0), 0);
        assert_eq!(map.to_utf16(3), 2);
        assert_eq!(map.to_utf16(7), 4);
        assert_eq!(map.to_utf16(8), 5);
        assert_eq!(map.to_byte(4), 7);
        assert_eq!(map.to_byte(99), 8);
    }

    #[test]
    fn analyzes_in_utf16_units() {
        let sql = "select '🦆';\nupdate t set x = 1".to_owned();
        let a = analyze_sql(sql, DriverKind::Postgres, 14);
        assert_eq!(a.statements.len(), 2);
        assert_eq!((a.statements[0].start, a.statements[0].end), (0, 11));
        assert!(a.statements[0].read_only);
        assert_eq!(a.statements[1].start, 13);
        assert!(!a.statements[1].read_only);
        assert_eq!(a.at_cursor, Some(1));
    }

    #[test]
    fn reports_parameters_and_lint_in_utf16_units() {
        let sql = "@set x = 1\nselect '🦆', :id from t join u".to_owned();
        let a = analyze_sql(sql.clone(), DriverKind::Postgres, 0);
        assert_eq!(a.statements[0].kind, "command");
        assert!(a.statements[0].read_only);
        let p = &a.statements[1].parameters[0];
        assert_eq!(
            (p.kind, p.name.as_str(), p.start, p.end),
            ("named", "id", 24, 27)
        );
        let lint = lint_sql(sql.clone(), DriverKind::Postgres);
        assert!(lint_sql(sql, DriverKind::Mongo).issues.is_empty());
        assert_eq!(lint.statements.len(), 1);
        assert_eq!(lint.issues[0].kind, "join_without_condition");
        assert_eq!(lint.statements[0].tables[0].name.as_deref(), Some("t"));
    }

    #[test]
    fn tells_what_a_read_only_connection_runs() {
        assert!(only_reads("select 1; select 2", DriverKind::Postgres));
        assert!(!only_reads("select 1; delete from t", DriverKind::Postgres));
        assert!(!only_reads("set search_path = x", DriverKind::Postgres));
        assert!(!only_reads("SELECT 1 EXEC('x')", DriverKind::SqlServer));
        assert!(only_reads("db.a.find()\nshow dbs", DriverKind::Mongo));
        assert!(!only_reads("db.a.find()\ndb.a.drop()", DriverKind::Mongo));
        assert!(!only_reads("db.a.find(", DriverKind::Mongo));
    }

    /// The maintenance statements the Maintenance dialog runs, typed in the editor, a script,
    /// a schedule or the command-line client: none runs on a read-only connection.
    #[test]
    fn maintenance_is_no_read() {
        for (driver, sql) in [
            (DriverKind::Postgres, "VACUUM (VERBOSE) demo.t"),
            (DriverKind::Postgres, "VACUUM FULL"),
            (DriverKind::Postgres, "ANALYZE demo.t"),
            (DriverKind::Postgres, "REINDEX TABLE demo.t"),
            (DriverKind::Postgres, "CLUSTER demo.t"),
            (DriverKind::MySql, "OPTIMIZE TABLE t"),
            (DriverKind::MySql, "ANALYZE TABLE t"),
            (DriverKind::MySql, "CHECK TABLE t"),
            (DriverKind::MySql, "REPAIR TABLE t"),
            (DriverKind::SqlServer, "UPDATE STATISTICS dbo.t"),
            (DriverKind::SqlServer, "ALTER INDEX ALL ON dbo.t REBUILD"),
            (DriverKind::SqlServer, "DBCC CHECKTABLE ('dbo.t')"),
            (DriverKind::Sqlite, "VACUUM"),
            (DriverKind::Sqlite, "REINDEX t"),
        ] {
            assert!(!only_reads(sql, driver), "{driver:?}: {sql}");
        }
    }

    #[test]
    fn sql_server_statements_keep_their_semicolon_and_batches() {
        let sql = "@set x = 1;\nDECLARE @a int = 1;\nMERGE t USING s ON 1 = 1 WHEN MATCHED THEN DELETE;\nGO 2\nSELECT 1"
            .to_owned();
        let a = analyze_sql(sql.clone(), DriverKind::SqlServer, 0);
        let text = |i: usize| &sql[a.statements[i].start..a.statements[i].end];
        assert_eq!(text(0), "@set x = 1");
        assert_eq!(text(1), "DECLARE @a int = 1;");
        assert!(text(2).ends_with("DELETE;"));
        let batches: Vec<_> = a
            .batches
            .iter()
            .map(|b| (b.first, b.end, b.repeat))
            .collect();
        assert_eq!(batches, [(0, 1, 1), (1, 3, 2), (3, 4, 1)]);
        assert!(
            analyze_sql("select 1;".to_owned(), DriverKind::Postgres, 0)
                .batches
                .is_empty()
        );
    }

    #[test]
    fn analyzes_redis_commands() {
        let src = "GET a\n# note\nSET b 1\nBROWSE db2.hash".to_owned();
        let a = analyze_sql(src, DriverKind::Redis, 7);
        let kinds: Vec<_> = a.statements.iter().map(|s| (s.kind, s.read_only)).collect();
        assert_eq!(kinds, [("query", true), ("dml", false), ("query", true)]);
        assert_eq!(a.at_cursor, Some(0));
        let flush = analyze_sql("FLUSHALL\nCONFIG SET a b".to_owned(), DriverKind::Redis, 0);
        assert_eq!(flush.statements[0].unfiltered[0].verb, "FLUSHALL");
        assert_eq!(flush.statements[1].kind, "ddl");
        assert!(only_reads("GET a\nHGETALL h", DriverKind::Redis));
        assert!(!only_reads("GET a\nDEL a", DriverKind::Redis));
        assert!(!only_reads("GET \"a", DriverKind::Redis));
        let source = query_source("BROWSE db2.hash".to_owned(), DriverKind::Redis).unwrap();
        assert_eq!(
            (source.schema.as_deref(), source.name.as_str()),
            (Some("db2"), "hash")
        );
        assert!(query_source("GET a".to_owned(), DriverKind::Redis).is_none());
        assert!(sql_count_query("BROWSE hash".to_owned(), DriverKind::Redis).is_none());
        assert!(
            lint_sql("SELECT * FROM t".to_owned(), DriverKind::Redis)
                .issues
                .is_empty()
        );
    }

    #[test]
    fn cql_comments_and_dollar_strings_hide_nothing_and_make_no_parameters() {
        let src = "// note ? here\nTRUNCATE shop.t; CREATE FUNCTION f(x int) RETURNS NULL ON NULL INPUT \
                   RETURNS int LANGUAGE java AS $$ return x > 0 ? 1 : 2; $$; SELECT * FROM t WHERE id = ?"
            .to_owned();
        let a = analyze_sql(src.clone(), DriverKind::Cassandra, 0);
        assert_eq!(a.statements.len(), 3);
        assert_eq!(a.statements[0].unfiltered.len(), 1);
        assert!(a.statements[0].parameters.is_empty());
        assert!(a.statements[1].parameters.is_empty());
        let marks = &a.statements[2].parameters;
        assert_eq!(marks.len(), 1);
        assert_eq!(&src[marks[0].start..marks[0].end], "?");
    }

    #[test]
    fn analyzes_cql_scripts() {
        let src = "SELECT * FROM shop.t WHERE id = 1; -- a; b\nTRUNCATE shop.t;\n\
                   BEGIN BATCH INSERT INTO t (id) VALUES (1); APPLY BATCH;\nUPDATE t SET v = 1 WHERE id = 2"
            .to_owned();
        let a = analyze_sql(src.clone(), DriverKind::Cassandra, 5);
        let kinds: Vec<_> = a.statements.iter().map(|s| (s.kind, s.read_only)).collect();
        assert_eq!(
            kinds,
            [
                ("query", true),
                ("ddl", false),
                ("ddl", false),
                ("dml", false)
            ]
        );
        assert_eq!(a.at_cursor, Some(0));
        let every: Vec<_> = a
            .statements
            .iter()
            .flat_map(|s| s.unfiltered.iter().map(|t| (t.verb, t.name.as_str())))
            .collect();
        assert_eq!(every, [("TRUNCATE", "t")]);
        // The batch is one statement, its `;` inside.
        let batch = &a.statements[2];
        assert!(src[batch.start..batch.end].ends_with("APPLY BATCH"));
        assert_eq!(
            analyze_sql(src.clone(), DriverKind::Cassandra, src.len()).at_cursor,
            Some(3)
        );
        assert!(only_reads(
            "SELECT 1; SELECT * FROM t LIMIT 3",
            DriverKind::Cassandra
        ));
        assert!(!only_reads(
            "SELECT 1; DELETE FROM t WHERE id = 1",
            DriverKind::Cassandra
        ));
        assert!(!only_reads(
            "BEGIN BATCH APPLY BATCH",
            DriverKind::Cassandra
        ));
        assert!(!only_reads("USE shop", DriverKind::Cassandra));
        assert!(!only_reads("// x\nTRUNCATE t", DriverKind::Cassandra));
        assert!(only_reads("// x\nSELECT 1", DriverKind::Cassandra));
        let source = query_source(
            "SELECT * FROM shop.t LIMIT 10".to_owned(),
            DriverKind::Cassandra,
        )
        .unwrap();
        assert_eq!(
            (source.schema.as_deref(), source.name.as_str()),
            (Some("shop"), "t")
        );
        // The viewer's rewrites would write CQL the server refuses.
        assert!(sql_count_query("SELECT * FROM t".to_owned(), DriverKind::Cassandra).is_none());
        assert!(sql_add_limit("SELECT * FROM t".to_owned(), DriverKind::Cassandra, 5).is_none());
        assert!(!sql_sortable(
            "SELECT * FROM t".to_owned(),
            DriverKind::Cassandra
        ));
    }

    #[test]
    fn analyzes_search_requests() {
        let src = "GET /orders/_search\n{\"size\":1}\n\nPOST /orders/_delete_by_query\n\n\
                   SELECT * FROM orders\n\nDELETE /orders"
            .to_owned();
        let a = analyze_sql(src.clone(), DriverKind::Elasticsearch, 0);
        let kinds: Vec<_> = a.statements.iter().map(|s| (s.kind, s.read_only)).collect();
        assert_eq!(
            kinds,
            [
                ("query", true),
                ("dml", false),
                ("query", true),
                ("ddl", false)
            ]
        );
        let every: Vec<_> = a
            .statements
            .iter()
            .flat_map(|s| s.unfiltered.iter().map(|t| (t.verb, t.name.as_str())))
            .collect();
        assert_eq!(every, [("DELETE BY QUERY", "orders")]);
        assert_eq!(a.at_cursor, Some(0));
        assert!(only_reads(
            "GET /a/_search\nSELECT 1",
            DriverKind::Elasticsearch
        ));
        assert!(!only_reads(
            "GET /a/_search\nDELETE /a",
            DriverKind::Elasticsearch
        ));
        // A path that would resolve to a write is no read: it does not parse.
        assert!(!only_reads(
            "POST /a/_search/../_bulk\n{\"delete\":{\"_id\":\"1\"}}",
            DriverKind::Elasticsearch
        ));
        let source = query_source(
            "GET /orders/_search\n{\"query\":{\"match_all\":{}}}".to_owned(),
            DriverKind::Elasticsearch,
        )
        .unwrap();
        assert_eq!(source.name, "orders");
        assert!(query_source("SELECT 1".to_owned(), DriverKind::Elasticsearch).is_none());
        assert!(sql_count_query("SELECT 1".to_owned(), DriverKind::Elasticsearch).is_none());
        assert!(
            lint_sql("SELECT * FROM t".to_owned(), DriverKind::Elasticsearch)
                .issues
                .is_empty()
        );
    }

    #[test]
    fn splits_mongosh_scripts() {
        let src = "db.a.find()\ndb.b.deleteMany({})\nshow dbs".to_owned();
        let a = analyze_sql(src.clone(), DriverKind::Mongo, src.len());
        let kinds: Vec<_> = a.statements.iter().map(|s| (s.kind, s.read_only)).collect();
        assert_eq!(kinds, [("query", true), ("dml", false), ("query", true)]);
        assert_eq!(a.at_cursor, Some(2));
        let every: Vec<_> = a.statements[1]
            .unfiltered
            .iter()
            .map(|t| (t.verb, t.name.as_str()))
            .collect();
        assert_eq!(every, [("deleteMany", "b")]);
        assert!(query_source(src, DriverKind::Mongo).is_none());
    }
}

/// Whether the data viewer may rewrite `sql`: a single read-only query whose top level it can
/// place changes in with certainty. The rewrites below return `None` for anything else.
fn rewritable(sql: &str, driver: DriverKind) -> bool {
    match driver {
        DriverKind::Mongo => true,
        // CQL has no `OFFSET` and takes `LIMIT` before `ALLOW FILTERING`, so the rewrites that
        // sort, count, cap and filter a query would write statements the server refuses.
        DriverKind::Redis | DriverKind::Elasticsearch | DriverKind::Cassandra => false,
        _ => platypus_sql::analyze(sql, Dialect::from(driver)).kind == StatementKind::Query,
    }
}

/// `sql` sorted by `column` (descending when `desc`) or, with no column, without the
/// viewer's sort: back to `original`, the `ORDER BY` it had before (see `sql_order_clause`),
/// or unsorted. With `position` (from 1), the sort names the column by its place in the
/// select list, for a column its name cannot point to (two `id` of a join, an expression).
/// The rest of the text stays as written. `None` when `sql` is not a read-only query the sort
/// can be placed in. The data viewer's header sort.
#[tauri::command]
pub fn sql_order_by(
    sql: String,
    driver: DriverKind,
    column: Option<String>,
    desc: bool,
    original: Option<String>,
    position: Option<usize>,
) -> Option<String> {
    let order = column.as_deref().map(|c| (c, desc));
    if driver == DriverKind::Mongo {
        return platypus_driver_mongo::viewer::set_sort(&sql, order);
    }
    if !rewritable(&sql, driver) {
        return None;
    }
    let dialect = Dialect::from(driver);
    match (order, original, position) {
        (None, Some(clause), _) => platypus_sql::restore_order_by(&sql, dialect, &clause),
        (Some(_), _, Some(position)) => {
            platypus_sql::set_order_by_position(&sql, dialect, position, desc)
        }
        _ => platypus_sql::set_order_by(&sql, dialect, order),
    }
}

/// The top-level `ORDER BY` of `sql` as written, which the header sort puts back after
/// descending; `None` when it has none, or on MongoDB.
#[tauri::command]
pub fn sql_order_clause(sql: String, driver: DriverKind) -> Option<String> {
    if driver == DriverKind::Mongo || !rewritable(&sql, driver) {
        return None;
    }
    platypus_sql::order_by_clause(&sql, Dialect::from(driver))
}

/// Whether the header sort can rewrite `sql`.
#[tauri::command]
pub fn sql_sortable(sql: String, driver: DriverKind) -> bool {
    sql_order_by(sql, driver, Some("x".to_owned()), false, None, None).is_some()
}

/// The query that counts the rows of `sql` (`SELECT COUNT(*)…`, or `countDocuments`); `None`
/// when it is not a read-only query that can be counted.
#[tauri::command]
pub fn sql_count_query(sql: String, driver: DriverKind) -> Option<String> {
    if driver == DriverKind::Mongo {
        return platypus_driver_mongo::viewer::count(&sql);
    }
    if !rewritable(&sql, driver) {
        return None;
    }
    platypus_sql::count_query(&sql, Dialect::from(driver))
}

/// `sql` capped at `limit` rows (`LIMIT`, `TOP`, `FETCH FIRST`); `None` when it is not a
/// read-only query on a table without a row cap of its own. The editor's automatic limit.
#[tauri::command]
pub fn sql_add_limit(sql: String, driver: DriverKind, limit: u64) -> Option<String> {
    if driver == DriverKind::Mongo || !rewritable(&sql, driver) {
        return None;
    }
    platypus_sql::add_limit(&sql, Dialect::from(driver), limit)
}

/// `sql` with `condition` added with AND to its top-level `WHERE`; `None` when it is not a read-only
/// query the condition can be placed in. The grid's quick filters.
#[tauri::command]
pub fn sql_add_condition(sql: String, driver: DriverKind, condition: String) -> Option<String> {
    if !rewritable(&sql, driver) || driver == DriverKind::Mongo {
        return None;
    }
    platypus_sql::add_condition(&sql, Dialect::from(driver), &condition)
}

/// What a quick filter on a result column names in the `WHERE` (see
/// `platypus_sql::FilterColumn`): `{ kind: "column", sql }`, `{ kind: "computed" }` or
/// `{ kind: "ambiguous" }`.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "sql", rename_all = "snake_case")]
pub enum FilterTarget {
    Column(String),
    Computed,
    Ambiguous,
}

/// For each column of the result of `sql`, named `columns`, what a quick filter on it names
/// in the `WHERE`: the column it shows (`name` for `name AS n`), or why there is none. `None`
/// overall when no condition can be added to `sql`.
#[tauri::command]
pub fn sql_filter_columns(
    sql: String,
    driver: DriverKind,
    columns: Vec<String>,
) -> Option<Vec<FilterTarget>> {
    if !rewritable(&sql, driver) || driver == DriverKind::Mongo {
        return None;
    }
    let names: Vec<&str> = columns.iter().map(String::as_str).collect();
    let targets = platypus_sql::filter_columns(&sql, Dialect::from(driver), &names)?;
    Some(
        targets
            .into_iter()
            .map(|target| match target {
                FilterColumn::Column(sql) => FilterTarget::Column(sql),
                FilterColumn::Computed => FilterTarget::Computed,
                FilterColumn::Ambiguous => FilterTarget::Ambiguous,
            })
            .collect(),
    )
}
