//! What the call log keeps about a call for the agent report: how many rows it returned,
//! which tables it read and whether it changed anything or was refused when it tried. Names
//! and counts only; a statement or a value never reaches the log through here.

use platypus_core::DriverKind;
use platypus_sql::{Dialect, Lexer, TokenKind};
use platypus_store::{CallEffect, CallFacts};

use crate::Agent;
use crate::access;
use crate::error::AgentError;

/// What a tool call touched, as far as its arguments say.
#[derive(Debug, Clone, Default)]
pub(crate) enum Probe {
    /// Nothing the report counts (listing, planning, proposing).
    #[default]
    Nothing,
    /// A table whose rows the call returns (`sample_rows`, `profile_table`).
    Table { schema: String, table: String },
    /// A statement on a connection (`run_query`).
    Statement { connection: String, sql: String },
    /// Tables of several connections joined by a statement (`query_across_databases`).
    Tables(Vec<String>),
    /// More rows of a result already counted (`next_page`).
    Page,
}

/// `schema.table`, the name the report keeps.
fn qualified(schema: &str, table: &str) -> String {
    if schema.is_empty() {
        table.to_owned()
    } else {
        format!("{schema}.{table}")
    }
}

impl Probe {
    pub(crate) fn table(schema: &str, table: &str) -> Self {
        Self::Table {
            schema: schema.to_owned(),
            table: table.to_owned(),
        }
    }

    pub(crate) fn statement(connection: &str, sql: &str) -> Self {
        Self::Statement {
            connection: connection.to_owned(),
            sql: sql.to_owned(),
        }
    }

    pub(crate) fn sources<'a>(sources: impl Iterator<Item = (&'a str, &'a str)>) -> Self {
        Self::Tables(sources.map(|(s, t)| qualified(s, t)).collect())
    }
}

/// The rows a tool answer reports: the JSON `row_count`, or the footer line `(N rows, ...`
/// of the text table; 0 for an answer that has neither (a change's `N rows affected.`).
pub(crate) fn rows_in(answer: &str) -> u64 {
    if let Some(at) = answer.find("\"row_count\":") {
        let digits: String = answer[at + 12..]
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        return digits.parse().unwrap_or(0);
    }
    answer
        .lines()
        .rev()
        .take(8)
        .find_map(|line| {
            let rest = line.strip_prefix('(')?;
            let (n, after) = rest.split_once(' ')?;
            after.starts_with("row").then(|| n.parse().ok())?
        })
        .unwrap_or(0)
}

/// Words that end a `FROM` list.
const CLAUSES: &[&str] = &[
    "WHERE",
    "GROUP",
    "ORDER",
    "HAVING",
    "LIMIT",
    "UNION",
    "INTERSECT",
    "EXCEPT",
    "ON",
    "USING",
    "SELECT",
    "WINDOW",
    "QUALIFY",
    "OFFSET",
    "FETCH",
    "FOR",
    "RETURNING",
    "SET",
    "VALUES",
];

/// Words that follow `FROM` or `JOIN` without being a table.
const NOT_TABLES: &[&str] = &[
    "SELECT", "LATERAL", "UNNEST", "VALUES", "TABLE", "WITH", "ONLY",
];

/// Functions whose parentheses hold a `FROM` that is not a table.
const FROM_FUNCTIONS: &[&str] = &["EXTRACT", "TRIM", "SUBSTRING", "OVERLAY", "POSITION"];

/// An identifier without its quotes.
fn unquoted(text: &str, kind: TokenKind) -> String {
    if kind == TokenKind::QuotedIdent {
        text.trim_matches(|c| matches!(c, '"' | '`' | '[' | ']'))
            .to_owned()
    } else {
        text.to_owned()
    }
}

/// The tables `sql` reads, an estimate: the names after `FROM` and `JOIN` (and after the
/// commas of a `FROM` list), in subqueries too, minus the statement's own `WITH` names.
/// A table function, a derived table and a name built by the statement are not found.
pub(crate) fn tables_of(sql: &str, dialect: Dialect) -> Vec<String> {
    let tokens: Vec<_> = Lexer::new(sql, dialect)
        .filter(|t| !t.is_trivia() && t.kind != TokenKind::ExecComment)
        .collect();
    let word = |i: usize| {
        tokens
            .get(i)
            .filter(|t| t.kind == TokenKind::Word)
            .map(|t| t.text(sql).to_ascii_uppercase())
    };
    let symbol = |i: usize, s: &str| {
        tokens
            .get(i)
            .is_some_and(|t| t.kind == TokenKind::Symbol && t.text(sql) == s)
    };
    let is_name = |i: usize| {
        tokens
            .get(i)
            .is_some_and(|t| matches!(t.kind, TokenKind::Word | TokenKind::QuotedIdent))
    };
    // Per level of parentheses: whether its `FROM` lists tables, and whether a `FROM` list
    // is being read.
    let mut levels: Vec<(bool, bool)> = vec![(true, false)];
    let mut found: Vec<String> = Vec::new();
    let mut with_names: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let prev = i.checked_sub(1).and_then(word);
        if symbol(i, "(") {
            let function = prev.as_deref().is_some_and(|w| FROM_FUNCTIONS.contains(&w));
            levels.push((!function, false));
        } else if symbol(i, ")") {
            if levels.len() > 1 {
                levels.pop();
            }
        } else if let Some(w) = word(i) {
            // `name AS (` is a `WITH` name, not a table.
            if word(i + 1).as_deref() == Some("AS") && symbol(i + 2, "(") {
                with_names.push(w.clone());
            }
            let level = levels.last().is_none_or(|l| l.0);
            let start = match w.as_str() {
                "FROM" if level && prev.as_deref() != Some("DISTINCT") => Some(true),
                "JOIN" if prev.as_deref() != Some("ARRAY") => Some(false),
                _ => None,
            };
            if let Some(list) = start {
                if let Some(l) = levels.last_mut() {
                    l.1 = list;
                }
                i = read_table(sql, &tokens, i + 1, &mut found);
                continue;
            }
            if CLAUSES.contains(&w.as_str())
                && let Some(l) = levels.last_mut()
            {
                l.1 = false;
            }
        } else if symbol(i, ",") && levels.last().is_some_and(|l| l.1) && is_name(i + 1) {
            i = read_table(sql, &tokens, i + 1, &mut found);
            continue;
        }
        i += 1;
    }
    let mut seen: Vec<String> = Vec::new();
    for name in found {
        let own = !name.contains('.') && with_names.iter().any(|w| w.eq_ignore_ascii_case(&name));
        if !own && !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

/// Reads the dotted table name at `tokens[from..]` into `found`; returns the index after it
/// (or `from` when there is no table there: a subquery, a function).
fn read_table(
    sql: &str,
    tokens: &[platypus_sql::Token],
    from: usize,
    found: &mut Vec<String>,
) -> usize {
    let mut i = from;
    let mut parts: Vec<String> = Vec::new();
    while let Some(t) = tokens.get(i)
        && matches!(t.kind, TokenKind::Word | TokenKind::QuotedIdent)
    {
        parts.push(unquoted(t.text(sql), t.kind));
        i += 1;
        let dot = tokens
            .get(i)
            .is_some_and(|t| t.kind == TokenKind::Symbol && t.text(sql) == ".");
        if !dot {
            break;
        }
        i += 1;
    }
    let called = tokens
        .get(i)
        .is_some_and(|t| t.kind == TokenKind::Symbol && t.text(sql) == "(");
    let keyword = parts.len() == 1 && NOT_TABLES.iter().any(|w| w.eq_ignore_ascii_case(&parts[0]));
    if parts.is_empty() || called || keyword {
        return from.max(i);
    }
    found.push(parts.join("."));
    i
}

/// Whether `sql` changes anything on a connection of `driver`, and if so how the call ended.
fn effect_of(sql: &str, driver: DriverKind, ended: Ended) -> CallEffect {
    if !access::changes(sql, driver) {
        return CallEffect::None;
    }
    match ended {
        Ended::Answered => CallEffect::Allowed,
        Ended::Refused => CallEffect::Refused,
        Ended::Failed => CallEffect::None,
    }
}

/// How a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended {
    Answered,
    /// The access, a safety setting or the user said no.
    Refused,
    Failed,
}

impl Ended {
    pub(crate) fn of<T>(result: &Result<T, AgentError>) -> Self {
        match result {
            Ok(_) => Self::Answered,
            Err(AgentError::Denied(_)) => Self::Refused,
            Err(_) => Self::Failed,
        }
    }
}

/// Whether the driver speaks SQL the lexer can read.
fn speaks_sql(driver: DriverKind) -> bool {
    !matches!(
        driver,
        DriverKind::Mongo | DriverKind::Redis | DriverKind::Elasticsearch | DriverKind::Cassandra
    )
}

impl Agent {
    /// The facts of a call that touched `probe`, returned `rows` and `ended` as said. The
    /// connection of a statement is looked up here, off the answer's way.
    pub(crate) fn call_facts(&self, probe: &Probe, rows: u64, ended: Ended) -> CallFacts {
        let reads = |tables: Vec<String>| CallFacts {
            rows,
            tables: if ended == Ended::Answered {
                tables
            } else {
                Vec::new()
            },
            effect: CallEffect::None,
        };
        match probe {
            Probe::Nothing => CallFacts::default(),
            Probe::Page => reads(Vec::new()),
            Probe::Table { schema, table } => reads(vec![qualified(schema, table)]),
            Probe::Tables(names) => reads(names.clone()),
            Probe::Statement { connection, sql } => match self.find(connection) {
                Ok((_, saved)) => statement_facts(sql, saved.driver, rows, ended),
                Err(_) => CallFacts::default(),
            },
        }
    }
}

/// The facts of a call to a statement tool on a connection of `driver`.
pub(crate) fn statement_facts(sql: &str, driver: DriverKind, rows: u64, ended: Ended) -> CallFacts {
    let effect = effect_of(sql, driver, ended);
    let tables = if effect == CallEffect::None && ended == Ended::Answered && speaks_sql(driver) {
        tables_of(sql, Dialect::from(driver))
    } else {
        Vec::new()
    };
    CallFacts {
        rows,
        tables,
        effect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables(sql: &str) -> Vec<String> {
        tables_of(sql, Dialect::Postgres)
    }

    #[test]
    fn finds_the_tables_after_from_and_join() {
        assert_eq!(tables("SELECT * FROM orders"), ["orders"]);
        assert_eq!(
            tables(
                "select 1 from public.orders o, items i join \"Users\" u on u.id = o.uid where 1=1"
            ),
            ["public.orders", "items", "Users"]
        );
        assert_eq!(
            tables(
                "SELECT * FROM (SELECT * FROM a) s JOIN b USING (id) WHERE x IN (SELECT y FROM c)"
            ),
            ["a", "b", "c"]
        );
        assert_eq!(tables("SELECT 1"), Vec::<String>::new());
    }

    #[test]
    fn ignores_what_is_not_a_table() {
        // A WITH name, the FROM inside EXTRACT, a table function, IS DISTINCT FROM.
        assert_eq!(
            tables("WITH w AS (SELECT * FROM t) SELECT extract(year FROM d), * FROM w"),
            ["t"]
        );
        assert_eq!(
            tables("SELECT * FROM generate_series(1, 3) g"),
            Vec::<String>::new()
        );
        assert_eq!(tables("SELECT a IS DISTINCT FROM b FROM t"), ["t"]);
        // A comma in a WHERE list is not another table.
        assert_eq!(tables("SELECT * FROM t WHERE a IN (1, 2) AND b = 3"), ["t"]);
        assert_eq!(
            tables("SELECT * FROM t1, t2 WHERE t1.a = t2.a"),
            ["t1", "t2"]
        );
    }

    #[test]
    fn counts_the_rows_an_answer_reports() {
        assert_eq!(rows_in("a\n-\n1\n(1 row, 3 ms)"), 1);
        assert_eq!(
            rows_in("a\n-\n1\n(25 rows shown, 3 ms: the result has more.)"),
            25
        );
        assert_eq!(rows_in(r#"{"columns":[],"row_count":42,"rows":[]}"#), 42);
        assert_eq!(rows_in("2 rows affected."), 0);
    }

    #[test]
    fn a_change_is_allowed_or_refused_by_how_the_call_ended() {
        let sql = "DELETE FROM t WHERE id = 1";
        let driver = DriverKind::Sqlite;
        assert_eq!(effect_of(sql, driver, Ended::Answered), CallEffect::Allowed);
        assert_eq!(effect_of(sql, driver, Ended::Refused), CallEffect::Refused);
        assert_eq!(effect_of(sql, driver, Ended::Failed), CallEffect::None);
        assert_eq!(
            effect_of("SELECT 1", driver, Ended::Refused),
            CallEffect::None
        );
        let denied: Result<(), _> = Err(AgentError::Denied("no".into()));
        assert_eq!(Ended::of(&denied), Ended::Refused);
        let broken: Result<(), _> = Err(AgentError::Database("no".into()));
        assert_eq!(Ended::of(&broken), Ended::Failed);
    }

    #[test]
    fn a_change_reads_no_tables() {
        let facts = statement_facts(
            "DELETE FROM t WHERE id IN (SELECT id FROM u)",
            DriverKind::Sqlite,
            0,
            Ended::Answered,
        );
        assert_eq!(facts.tables, Vec::<String>::new());
        assert_eq!(facts.effect, CallEffect::Allowed);
    }
}
