//! SQL text analysis for the editor.

use platypus_core::DriverKind;
use platypus_sql::{
    Dialect, FormatOptions, KeywordCase, LintKind, ParameterKind, SelectItem, StatementKind,
};
use serde::{Deserialize, Serialize};

/// A table a statement changes in every row; `schema` is `None` for the session's.
#[derive(Debug, Serialize)]
pub struct UnfilteredTable {
    schema: Option<String>,
    name: String,
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
    /// An `UPDATE` or `DELETE` without `WHERE`, or a `TRUNCATE`: its table.
    unfiltered: Option<UnfilteredTable>,
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

#[derive(Debug, Serialize)]
pub struct SqlAnalysis {
    statements: Vec<StatementSpan>,
    /// Index into `statements` of the one to run for the cursor.
    at_cursor: Option<usize>,
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
    let dialect = Dialect::from(driver);
    let statements = platypus_sql::split(&sql, dialect);
    let at = platypus_sql::statement_at(&sql, dialect, map.to_byte(cursor));
    SqlAnalysis {
        at_cursor: at.and_then(|at| statements.iter().position(|s| *s == at)),
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
                    end: map.to_utf16(s.end),
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
                    unfiltered: analysis.unfiltered.map(|t| UnfilteredTable {
                        schema: t.schema,
                        name: t.name,
                    }),
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

/// [`analyze_sql`] for mongosh statements, which have no parameters.
fn analyze_mongo(src: &str, map: &Utf16Map, cursor: usize) -> SqlAnalysis {
    use platypus_driver_mongo::shell::{self, StatementKind as Kind};
    let statements = shell::split(src);
    let at = shell::statement_at(src, map.to_byte(cursor));
    SqlAnalysis {
        at_cursor: at.and_then(|at| statements.iter().position(|s| *s == at)),
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
                    unfiltered: parsed.as_ref().and_then(shell::unfiltered).map(
                        |(schema, name)| UnfilteredTable {
                            schema: schema.map(str::to_owned),
                            name: name.to_owned(),
                        },
                    ),
                    parameters: Vec::new(),
                }
            })
            .collect(),
    }
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
    // The linter checks SQL; mongosh has none of its issues.
    if driver == DriverKind::Mongo {
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
    if driver == DriverKind::Mongo {
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
    fn splits_mongosh_scripts() {
        let src = "db.a.find()\ndb.b.deleteMany({})\nshow dbs".to_owned();
        let a = analyze_sql(src.clone(), DriverKind::Mongo, src.len());
        let kinds: Vec<_> = a.statements.iter().map(|s| (s.kind, s.read_only)).collect();
        assert_eq!(kinds, [("query", true), ("dml", false), ("query", true)]);
        assert_eq!(a.at_cursor, Some(2));
        assert_eq!(
            a.statements[1].unfiltered.as_ref().map(|t| t.name.as_str()),
            Some("b")
        );
        assert!(query_source(src, DriverKind::Mongo).is_none());
    }
}

/// Whether the data viewer may rewrite `sql`: a single read-only query whose top level it can
/// place changes in with certainty. The rewrites below return `None` for anything else.
fn rewritable(sql: &str, driver: DriverKind) -> bool {
    driver == DriverKind::Mongo
        || platypus_sql::analyze(sql, Dialect::from(driver)).kind == StatementKind::Query
}

/// `sql` sorted by `column` (descending when `desc`) or, with no column, without the
/// sort; the rest of the text stays as written. `None` when `sql` is not a read-only query
/// the sort can be placed in. The data viewer's header sort.
#[tauri::command]
pub fn sql_order_by(
    sql: String,
    driver: DriverKind,
    column: Option<String>,
    desc: bool,
) -> Option<String> {
    let order = column.as_deref().map(|c| (c, desc));
    if driver == DriverKind::Mongo {
        return platypus_driver_mongo::viewer::set_sort(&sql, order);
    }
    if !rewritable(&sql, driver) {
        return None;
    }
    platypus_sql::set_order_by(&sql, Dialect::from(driver), order)
}

/// Whether the header sort can rewrite `sql`.
#[tauri::command]
pub fn sql_sortable(sql: String, driver: DriverKind) -> bool {
    sql_order_by(sql, driver, Some("x".to_owned()), false).is_some()
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

/// `sql` with `condition` added with AND to its top-level `WHERE`; `None` when it is not a read-only
/// query the condition can be placed in. The grid's quick filters.
#[tauri::command]
pub fn sql_add_condition(sql: String, driver: DriverKind, condition: String) -> Option<String> {
    if !rewritable(&sql, driver) || driver == DriverKind::Mongo {
        return None;
    }
    platypus_sql::add_condition(&sql, Dialect::from(driver), &condition)
}
