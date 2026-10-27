//! What a statement does, checked before it runs: the app asks before changes that hit every
//! row of a table, and before any change on a production connection.

use platypus_core::{TxEffect, classify};

use crate::lexer::unquote_ident;
use crate::{Dialect, Lexer, Token, TokenKind, is_read_only};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    /// Reads only (see [`is_read_only`]).
    Query,
    /// Changes rows: `INSERT`, `UPDATE`, `DELETE`, `MERGE`, `CALL`, a `SELECT INTO`…
    Dml,
    /// Changes the schema or permissions: `CREATE`, `ALTER`, `DROP`, `TRUNCATE`, `GRANT`…
    Ddl,
    /// Neither, e.g. `BEGIN`, `SET`, `USE`.
    Other,
    /// A command the app runs itself, e.g. `@set x = 1`; the database never sees it.
    Command,
}

/// A table as a statement names it; `schema` is `None` when left to the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableName {
    pub schema: Option<String>,
    pub name: String,
}

/// A change to every row of a table: an `UPDATE` or `DELETE` without `WHERE` (or `LIMIT`),
/// or a `TRUNCATE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unfiltered {
    /// `UPDATE`, `DELETE` or `TRUNCATE`.
    pub verb: &'static str,
    /// The table changed, aliases resolved (`DELETE g FROM orders g JOIN …` changes `orders`).
    pub table: TableName,
    /// Transact-SQL's `TOP (n)`: up to that many rows, which the database picks.
    pub limited: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    pub kind: StatementKind,
    /// The changes to every row of a table, in order. A statement may hold several:
    /// `TRUNCATE a, b`, a Transact-SQL batch (which needs no `;`), or statements a MySQL
    /// `DELIMITER` joined.
    pub unfiltered: Vec<Unfiltered>,
    /// Begins or ends a transaction anywhere in it, not only at its start: `SELECT 1 COMMIT`
    /// in Transact-SQL, `…; COMMIT` after a MySQL `DELIMITER`.
    pub transaction: bool,
}

const DDL: &[&str] = &[
    "CREATE", "ALTER", "DROP", "TRUNCATE", "RENAME", "GRANT", "REVOKE", "COMMENT",
];
const OTHER: &[&str] = &[
    "BEGIN",
    "START",
    "COMMIT",
    "END",
    "ROLLBACK",
    "ABORT",
    "SAVEPOINT",
    "RELEASE",
    "SET",
    "RESET",
    "USE",
    "PRAGMA",
    "ANALYZE",
    "LISTEN",
    "NOTIFY",
    "DISCARD",
    "DEALLOCATE",
    "PREPARE",
];
/// Words between `UPDATE`/`DELETE` and the table.
const MODIFIERS: &[&str] = &[
    "LOW_PRIORITY",
    "QUICK",
    "IGNORE",
    "ONLY",
    "OR",
    "ROLLBACK",
    "ABORT",
    "REPLACE",
    "FAIL",
];
/// The main statement after a `WITH`'s common table expressions.
const MAIN_VERBS: &[&str] = &["SELECT", "INSERT", "UPDATE", "DELETE", "MERGE"];
/// The changes checked for every row.
const CHANGES: &[&str] = &["UPDATE", "DELETE", "TRUNCATE"];
/// Objects whose body runs later, not when they are created: its changes are not checked.
const ROUTINES: &[&str] = &[
    "PROCEDURE",
    "PROC",
    "FUNCTION",
    "TRIGGER",
    "EVENT",
    "PACKAGE",
    "TYPE",
    "RULE",
];
/// Transact-SQL words that start a statement of a batch: the statement before ends there.
/// (`SET` is not one here: it is part of `UPDATE`; nor `WITH`, part of table hints.)
const TSQL_STARTS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "MERGE", "TRUNCATE", "CREATE", "ALTER", "DROP", "EXEC",
    "EXECUTE", "DECLARE", "IF", "WHILE", "BEGIN", "COMMIT", "ROLLBACK", "SAVE", "USE", "PRINT",
    "WAITFOR", "GRANT", "REVOKE", "DENY", "RETURN",
];
/// Words before an `UPDATE` or `DELETE` that make it part of another clause (`FOR UPDATE`,
/// `AFTER UPDATE`, `GRANT UPDATE`, `WHEN MATCHED THEN DELETE`); `ON` too when a referential
/// action follows (`ON DELETE CASCADE`), not after `SET NOCOUNT ON`.
const CLAUSE_BEFORE: &[&str] = &[
    "FOR", "AFTER", "BEFORE", "OF", "INSTEAD", "THEN", "GRANT", "DENY", "REVOKE",
];
/// Referential actions, after `ON DELETE` or `ON UPDATE`.
const ACTIONS: &[&str] = &["CASCADE", "NO", "RESTRICT", "SET"];

pub(crate) struct Words<'a> {
    src: &'a str,
    pub(crate) tokens: Vec<Token>,
}

impl<'a> Words<'a> {
    /// The tokens of `sql` without whitespace and comments, reading the body of a MySQL
    /// executable comment (`/*! … */`) as the code it is.
    pub(crate) fn new(sql: &'a str, dialect: Dialect) -> Self {
        Self::with_lexer(sql, Lexer::executable(sql, dialect))
    }

    /// The tokens `lexer` reads in `sql`, without trivia and executable comment marks.
    fn with_lexer(sql: &'a str, lexer: Lexer<'a>) -> Self {
        Words {
            src: sql,
            tokens: lexer
                .filter(|t| !t.is_trivia() && t.kind != TokenKind::ExecComment)
                .collect(),
        }
    }

    pub(crate) fn word(&self, i: usize) -> Option<String> {
        let t = self.tokens.get(i)?;
        (t.kind == TokenKind::Word).then(|| t.text(self.src).to_ascii_uppercase())
    }

    pub(crate) fn is(&self, i: usize, words: &[&str]) -> bool {
        self.word(i).is_some_and(|w| words.contains(&w.as_str()))
    }

    pub(crate) fn symbol(&self, i: usize, s: &str) -> bool {
        self.tokens
            .get(i)
            .is_some_and(|t| t.kind == TokenKind::Symbol && t.text(self.src) == s)
    }

    pub(crate) fn ident(&self, i: usize, dialect: Dialect) -> Option<String> {
        let t = self.tokens.get(i)?;
        let text = t.text(self.src);
        match t.kind {
            TokenKind::Word if dialect == Dialect::Postgres => Some(text.to_lowercase()),
            TokenKind::Word => Some(text.to_owned()),
            TokenKind::QuotedIdent => unquote_ident(text),
            _ => None,
        }
    }

    /// `name` or `schema.name` at `i`.
    pub(crate) fn table(&self, i: usize, dialect: Dialect) -> Option<TableName> {
        let first = self.ident(i, dialect)?;
        if self.symbol(i + 1, ".") {
            let name = self.ident(i + 2, dialect)?;
            return Some(TableName {
                schema: Some(first),
                name,
            });
        }
        Some(TableName {
            schema: None,
            name: first,
        })
    }

    /// The indexes from `from` to `to` that are outside parentheses.
    fn top(&self, from: usize, to: usize) -> impl Iterator<Item = usize> + '_ {
        let mut depth = 0usize;
        (from..to.min(self.tokens.len())).filter(move |&i| {
            if self.symbol(i, "(") {
                depth += 1;
            } else if self.symbol(i, ")") {
                depth = depth.saturating_sub(1);
            }
            depth == 0
        })
    }

    /// The first word from `from` to `to`, outside parentheses, that is one of `words`.
    fn find_top(&self, from: usize, to: usize, words: &[&str]) -> Option<usize> {
        self.top(from, to).find(|&i| self.is(i, words))
    }

    /// The source text of the tokens from `from` to `to`.
    fn text(&self, from: usize, to: usize) -> &'a str {
        &self.src[self.tokens[from].start..self.tokens[to - 1].end]
    }
}

/// The table name at `i` and how many tokens it takes (`name`, `schema.name`). MySQL's `t.*`
/// in a `DELETE` target list counts as `t`.
fn table_at(w: &Words<'_>, i: usize, dialect: Dialect) -> Option<(TableName, usize)> {
    if w.symbol(i + 1, ".") && w.symbol(i + 2, "*") {
        let name = w.ident(i, dialect)?;
        return Some((TableName { schema: None, name }, 3));
    }
    let table = w.table(i, dialect)?;
    let len = if table.schema.is_some() { 3 } else { 1 };
    Some((table, len))
}

/// How many tokens Transact-SQL's `TOP (n) [PERCENT]` or `TOP n` takes at `i`; 0 without one.
fn top_clause(w: &Words<'_>, i: usize) -> usize {
    if !w.is(i, &["TOP"]) {
        return 0;
    }
    let mut j = i + 1;
    if w.symbol(j, "(") {
        j = w
            .top(j + 1, w.tokens.len())
            .find(|&k| w.symbol(k, ")"))
            .map_or(w.tokens.len(), |k| k + 1);
    } else if w.symbol(j, "@") {
        j += 2;
    } else {
        while w.tokens.get(j).is_some_and(|t| {
            t.kind == TokenKind::Symbol && t.text(w.src).chars().all(|c| c.is_ascii_digit())
        }) {
            j += 1;
        }
    }
    if w.is(j, &["PERCENT"]) {
        j += 1;
    }
    j - i
}

/// The table `name` stands for in the `FROM` list between `from` and `to`: the one it is the
/// alias of (`FROM orders o` for `o`), else `name` itself.
fn resolve(w: &Words<'_>, name: TableName, from: usize, to: usize, dialect: Dialect) -> TableName {
    let start = match &name.schema {
        None => w.find_top(from, to, &["FROM"]),
        Some(_) => None,
    };
    let Some(start) = start else {
        return name;
    };
    (start + 1..to)
        .find_map(|i| {
            let (table, len) = table_at(w, i, dialect)?;
            let alias = if w.is(i + len, &["AS"]) {
                i + len + 1
            } else {
                i + len
            };
            (alias < to
                && w.ident(alias, dialect)
                    .is_some_and(|a| a.eq_ignore_ascii_case(&name.name)))
            .then_some(table)
        })
        .unwrap_or(name)
}

/// The tables of a `TRUNCATE` whose names start at `i`: `a, b, ONLY c *`.
fn truncated(w: &Words<'_>, mut i: usize, dialect: Dialect) -> Vec<TableName> {
    if w.is(i, &["TABLE"]) {
        i += 1;
    }
    let mut tables = Vec::new();
    loop {
        while w.is(i, &["ONLY"]) {
            i += 1;
        }
        let Some((table, len)) = table_at(w, i, dialect) else {
            break;
        };
        tables.push(table);
        i += len;
        if w.symbol(i, "*") {
            i += 1;
        }
        if !w.symbol(i, ",") {
            break;
        }
        i += 1;
    }
    tables
}

/// The tables a `DELETE` whose targets start at `i` names: `FROM t` (and Transact-SQL's
/// `FROM g FROM orders g JOIN …`), `t FROM …`, MySQL's `a, b FROM a JOIN b …`, aliases
/// resolved in the `FROM` list before `end`.
fn deleted(w: &Words<'_>, mut i: usize, end: usize, dialect: Dialect) -> Vec<TableName> {
    let mut targets = Vec::new();
    if w.is(i, &["FROM"]) {
        i += 1;
        while w.is(i, &["ONLY"]) {
            i += 1;
        }
        if let Some((table, len)) = table_at(w, i, dialect) {
            targets.push(table);
            i += len;
        }
    } else {
        while let Some((table, len)) = table_at(w, i, dialect) {
            targets.push(table);
            i += len;
            if !w.symbol(i, ",") {
                break;
            }
            i += 1;
        }
    }
    targets
        .into_iter()
        .map(|t| resolve(w, t, i, end, dialect))
        .collect()
}

/// The tables an `UPDATE`, `DELETE` or `TRUNCATE` at `at` changes in every row; its clauses
/// end at `end`.
fn unfiltered(w: &Words<'_>, at: usize, end: usize, dialect: Dialect) -> Vec<Unfiltered> {
    let Some(verb) = w.word(at) else {
        return Vec::new();
    };
    let mut i = at + 1;
    while w.is(i, MODIFIERS) {
        i += 1;
    }
    let top = top_clause(w, i);
    i += top;
    let filtered = w.find_top(i, end, &["WHERE", "LIMIT"]).is_some();
    let (verb, tables): (&'static str, Vec<TableName>) = match verb.as_str() {
        "TRUNCATE" => ("TRUNCATE", truncated(w, i, dialect)),
        // `UPDATE(column)` is a trigger's function; `UPDATE STATISTICS` changes no row.
        "UPDATE" if filtered || w.symbol(i, "(") || w.is(i, &["STATISTICS"]) => return Vec::new(),
        "UPDATE" => (
            "UPDATE",
            table_at(w, i, dialect)
                .map(|(table, len)| resolve(w, table, i + len, end, dialect))
                .into_iter()
                .collect(),
        ),
        // MariaDB's `DELETE HISTORY FROM t` erases the past versions of a system-versioned
        // table, whatever `BEFORE SYSTEM_TIME` keeps.
        "DELETE"
            if dialect == Dialect::MySql && w.is(i, &["HISTORY"]) && w.is(i + 1, &["FROM"]) =>
        {
            if w.find_top(i, end, &["BEFORE"]).is_some() {
                return Vec::new();
            }
            ("DELETE HISTORY", deleted(w, i + 1, end, dialect))
        }
        "DELETE" if !filtered => ("DELETE", deleted(w, i, end, dialect)),
        _ => return Vec::new(),
    };
    tables
        .into_iter()
        .map(|table| Unfiltered {
            verb,
            table,
            limited: top > 0,
        })
        .collect()
}

/// Whether the Transact-SQL word at `i` begins or ends a transaction.
fn tsql_transaction(w: &Words<'_>, i: usize) -> bool {
    w.is(i, &["COMMIT", "ROLLBACK"])
        || (w.is(i, &["BEGIN"]) && w.is(i + 1, &["TRAN", "TRANSACTION", "DISTRIBUTED"]))
}

/// The changes to every row in a Transact-SQL batch between `start` and `end`: each change
/// that starts a statement, checked up to the next statement.
fn tsql_batch(w: &Words<'_>, main: usize, end: usize) -> Vec<Unfiltered> {
    let tops: Vec<usize> = w.top(main, end).collect();
    let starts_change = |i: usize| {
        i == main
            || (w.is(i, CHANGES)
                && !w.symbol(i - 1, ",")
                && !w.is(i - 1, CLAUSE_BEFORE)
                && (!w.is(i - 1, &["ON"]) || !w.is(i + 1, ACTIONS)))
    };
    let starts_statement =
        |i: usize| w.is(i, TSQL_STARTS) && (!w.is(i, CHANGES) || starts_change(i));
    let mut changes = Vec::new();
    for (n, &i) in tops.iter().enumerate() {
        if !starts_change(i) {
            continue;
        }
        let next = tops[n + 1..]
            .iter()
            .copied()
            .find(|&j| starts_statement(j))
            .unwrap_or(end);
        changes.extend(unfiltered(w, i, next, Dialect::SqlServer));
    }
    changes
}

/// The changes to every row in the statement between `start` and `end` (no `;` inside), and
/// whether it begins or ends a transaction.
fn part(w: &Words<'_>, (start, end): (usize, usize), dialect: Dialect) -> (Vec<Unfiltered>, bool) {
    let main = if w.is(start, &["WITH"]) {
        w.find_top(start + 1, end, MAIN_VERBS).unwrap_or(start)
    } else {
        start
    };
    if dialect == Dialect::SqlServer {
        let transaction = w.top(start, end).any(|i| tsql_transaction(w, i));
        return (tsql_batch(w, main, end), transaction);
    }
    let transaction = matches!(
        classify(w.text(start, end)),
        TxEffect::Begin | TxEffect::End
    );
    (unfiltered(w, main, end, dialect), transaction)
}

/// The changes to every row in `w` and whether it begins or ends a transaction, over the
/// statements its `;` separate. A routine's or trigger's body runs later: none of its
/// statements counts.
fn changes(w: &Words<'_>, dialect: Dialect) -> (Vec<Unfiltered>, bool) {
    let head: Vec<String> = (0..8).filter_map(|i| w.word(i)).collect();
    if matches!(head.first().map(String::as_str), Some("CREATE" | "ALTER"))
        && head.iter().any(|h| ROUTINES.contains(&h.as_str()))
    {
        return (Vec::new(), false);
    }
    let n = w.tokens.len();
    let mut start = 0;
    let mut all = (Vec::new(), false);
    for end in (0..=n).filter(|&i| i == n || w.symbol(i, ";")) {
        if start < end {
            let (changes, transaction) = part(w, (start, end), dialect);
            all.0.extend(changes);
            all.1 |= transaction;
        }
        start = end + 1;
    }
    all
}

#[must_use]
pub fn analyze(sql: &str, dialect: Dialect) -> Analysis {
    if sql.trim_start().starts_with('@') {
        return Analysis {
            kind: StatementKind::Command,
            unfiltered: Vec::new(),
            transaction: false,
        };
    }
    let first = Words::new(sql, dialect).word(0).unwrap_or_default();
    // An Oracle `BEGIN` with more after it is a PL/SQL block, which may change anything
    // (`DELETE`, `EXECUTE IMMEDIATE 'DROP …'`); a bare one is a transaction's start.
    let block = dialect == Dialect::Oracle
        && first == "BEGIN"
        && platypus_core::TxControl::of(sql).is_none();
    let kind = if is_read_only(sql, dialect) {
        StatementKind::Query
    } else if DDL.contains(&first.as_str()) {
        StatementKind::Ddl
    } else if OTHER.contains(&first.as_str()) && !block {
        StatementKind::Other
    } else {
        StatementKind::Dml
    };
    let mut unfiltered: Vec<Unfiltered> = Vec::new();
    let mut transaction = false;
    if kind != StatementKind::Query {
        // Whatever the server's settings: a change any reading finds is asked about.
        for lexer in Lexer::readings(sql, dialect) {
            let (changes, tx) = changes(&Words::with_lexer(sql, lexer), dialect);
            for change in changes {
                if !unfiltered.contains(&change) {
                    unfiltered.push(change);
                }
            }
            transaction |= tx;
        }
    }
    Analysis {
        kind,
        unfiltered,
        transaction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(schema: Option<&str>, name: &str) -> TableName {
        TableName {
            schema: schema.map(str::to_owned),
            name: name.to_owned(),
        }
    }

    /// The tables `sql` changes in every row, as `verb table`, `schema.table` when qualified.
    fn changes_in(sql: &str, dialect: Dialect) -> Vec<String> {
        analyze(sql, dialect)
            .unfiltered
            .iter()
            .map(|u| {
                let name = match &u.table.schema {
                    Some(schema) => format!("{schema}.{}", u.table.name),
                    None => u.table.name.clone(),
                };
                let top = if u.limited { " TOP" } else { "" };
                format!("{}{top} {name}", u.verb)
            })
            .collect()
    }

    fn unfiltered_of(sql: &str) -> Option<TableName> {
        analyze(sql, Dialect::Postgres)
            .unfiltered
            .first()
            .map(|u| u.table.clone())
    }

    #[test]
    fn finds_changes_to_every_row() {
        assert_eq!(
            unfiltered_of("UPDATE orders SET paid = true"),
            Some(table(None, "orders"))
        );
        assert_eq!(
            unfiltered_of("delete from shop.\"Orders\""),
            Some(table(Some("shop"), "Orders"))
        );
        assert_eq!(
            unfiltered_of("TRUNCATE TABLE ONLY logs"),
            Some(table(None, "logs"))
        );
        assert_eq!(
            unfiltered_of("WITH x AS (SELECT 1 WHERE true) DELETE FROM t"),
            Some(table(None, "t"))
        );
        // A WHERE inside a subquery does not filter the outer statement.
        assert_eq!(
            unfiltered_of("UPDATE t SET a = (SELECT max(b) FROM u WHERE u.id = 1)"),
            Some(table(None, "t"))
        );
        assert_eq!(
            changes_in("DELETE LOW_PRIORITY FROM `My Table`", Dialect::MySql),
            ["DELETE My Table"]
        );
        assert_eq!(
            changes_in("DELETE FROM [dbo].[My]]T]", Dialect::SqlServer),
            ["DELETE dbo.My]T"]
        );
    }

    #[test]
    fn names_every_table_changed() {
        let pg = Dialect::Postgres;
        assert_eq!(
            changes_in(
                "TRUNCATE TABLE qa_t1, ONLY s.qa_t2 *, qa_t3 RESTART IDENTITY",
                pg
            ),
            ["TRUNCATE qa_t1", "TRUNCATE s.qa_t2", "TRUNCATE qa_t3"]
        );
        assert_eq!(
            changes_in("WITH x AS (SELECT 1) DELETE FROM t", Dialect::Sqlite),
            ["DELETE t"]
        );
        let ms = Dialect::SqlServer;
        assert_eq!(
            changes_in("DELETE TOP (2) FROM qa_safe_g", ms),
            ["DELETE TOP qa_safe_g"]
        );
        assert_eq!(
            changes_in("UPDATE TOP 10 PERCENT g SET v = 1", ms),
            ["UPDATE TOP g"]
        );
        assert_eq!(
            changes_in(
                "UPDATE g SET v = 'x' FROM qa_safe_g g JOIN qa_safe_h h ON 1 = 1",
                ms
            ),
            ["UPDATE qa_safe_g"]
        );
        assert_eq!(
            changes_in(
                "DELETE g FROM dbo.qa_safe_g AS g JOIN qa_safe_h h ON 1 = 1",
                ms
            ),
            ["DELETE dbo.qa_safe_g"]
        );
        assert_eq!(
            changes_in("DELETE FROM g FROM qa_safe_g g", ms),
            ["DELETE qa_safe_g"]
        );
        let my = Dialect::MySql;
        // MariaDB's history of a system-versioned table, not a table named HISTORY.
        assert_eq!(
            changes_in("DELETE HISTORY FROM qa_dbs_sv", my),
            ["DELETE HISTORY qa_dbs_sv"]
        );
        assert_eq!(
            changes_in("DELETE HISTORY FROM t BEFORE SYSTEM_TIME '2020-01-01'", my),
            Vec::<String>::new()
        );
        assert_eq!(
            changes_in("DELETE g FROM qa_safe_g g JOIN qa_safe_h h", my),
            ["DELETE qa_safe_g"]
        );
        assert_eq!(
            changes_in("DELETE g.*, h FROM qa_safe_g g JOIN qa_safe_h h ON 1", my),
            ["DELETE qa_safe_g", "DELETE qa_safe_h"]
        );
        assert_eq!(
            changes_in("UPDATE qa_safe_g g JOIN qa_safe_h h ON 1 SET g.v = 1", my),
            ["UPDATE qa_safe_g"]
        );
        assert_eq!(
            changes_in("DELETE g FROM qa_safe_g g WHERE g.id = 1", my),
            Vec::<String>::new()
        );
    }

    #[test]
    fn finds_changes_in_batches() {
        let ms = Dialect::SqlServer;
        // A Transact-SQL batch needs no `;`: each change is its own statement.
        assert_eq!(
            changes_in("UPDATE t SET a = 1 WHERE id = 1 DELETE FROM u", ms),
            ["DELETE u"]
        );
        assert_eq!(
            changes_in("DELETE FROM u UPDATE t SET a = 1 WHERE id = 1", ms),
            ["DELETE u"]
        );
        assert_eq!(
            changes_in("SET NOCOUNT ON TRUNCATE TABLE t", ms),
            ["TRUNCATE t"]
        );
        assert_eq!(
            changes_in("DELETE FROM u SELECT 1 WHERE 1 = 1", ms),
            ["DELETE u"]
        );
        for sql in [
            "DELETE FROM t WITH (ROWLOCK) WHERE id = 1",
            "MERGE t USING u ON t.id = u.id WHEN MATCHED THEN DELETE WHEN NOT MATCHED THEN \
             INSERT (id) VALUES (u.id);",
            "GRANT SELECT, UPDATE ON t TO x",
            "ALTER TABLE t ADD FOREIGN KEY (a) REFERENCES u (a) ON DELETE CASCADE",
            "CREATE TRIGGER x ON t AFTER INSERT, UPDATE AS BEGIN UPDATE u SET a = 1 END",
            "UPDATE STATISTICS t",
            "DECLARE c CURSOR FOR SELECT a FROM t FOR UPDATE",
        ] {
            assert_eq!(changes_in(sql, ms), Vec::<String>::new(), "{sql}");
        }
        // Statements a MySQL DELIMITER joined.
        assert_eq!(
            changes_in(
                "UPDATE t SET a = 1 WHERE id = 1; DELETE FROM u",
                Dialect::MySql
            ),
            ["DELETE u"]
        );
        // A routine's body runs later.
        assert_eq!(
            changes_in(
                "CREATE PROCEDURE p() BEGIN DELETE FROM u; END",
                Dialect::MySql
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            changes_in(
                "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE b SET n = 1; END",
                Dialect::Sqlite
            ),
            Vec::<String>::new()
        );
        // Whatever the server's string settings: NO_BACKSLASH_ESCAPES ends this string early,
        // and the WHERE is then in a comment.
        assert_eq!(
            changes_in(
                r"UPDATE t SET a = 'x\'; DELETE FROM u; -- ' WHERE id = 1",
                Dialect::MySql
            ),
            ["UPDATE t", "DELETE u"]
        );
    }

    #[test]
    fn finds_transactions_anywhere() {
        let tx = |sql, dialect| analyze(sql, dialect).transaction;
        let ms = Dialect::SqlServer;
        assert!(tx("SELECT 1 COMMIT", ms));
        assert!(tx("UPDATE t SET a = 1 WHERE id = 1 BEGIN TRAN", ms));
        assert!(tx("DELETE FROM t WHERE id = 1 ROLLBACK", ms));
        assert!(!tx("IF 1 = 1 BEGIN DELETE FROM t WHERE id = 1 END", ms));
        assert!(!tx(
            "BEGIN TRY SELECT 1 END TRY BEGIN CATCH SELECT 2 END CATCH",
            ms
        ));
        let my = Dialect::MySql;
        assert!(tx("UPDATE t SET a = 1 WHERE id = 1; COMMIT", my));
        assert!(tx("START TRANSACTION", my));
        assert!(!tx("UPDATE t SET a = 1 WHERE id = 1", my));
    }

    #[test]
    fn leaves_filtered_changes_alone() {
        for sql in [
            "UPDATE t SET a = 1 WHERE id = 2",
            "DELETE FROM t WHERE id IN (SELECT id FROM u)",
            "UPDATE t SET a = 1 LIMIT 10",
            "INSERT INTO t VALUES (1)",
            "SELECT * FROM t",
        ] {
            assert_eq!(unfiltered_of(sql), None, "{sql}");
        }
    }

    #[test]
    fn classifies_statements() {
        let kind = |sql| analyze(sql, Dialect::Postgres).kind;
        assert_eq!(kind("select 1"), StatementKind::Query);
        assert_eq!(kind("insert into t values (1)"), StatementKind::Dml);
        assert_eq!(
            kind("with x as (select 1) update t set a = 1"),
            StatementKind::Dml
        );
        assert_eq!(kind("select 1 into t2"), StatementKind::Dml);
        assert_eq!(kind("create table t (a int)"), StatementKind::Ddl);
        assert_eq!(kind("drop table t"), StatementKind::Ddl);
        assert_eq!(kind("begin"), StatementKind::Other);
        assert_eq!(kind("set search_path = x"), StatementKind::Other);
        assert_eq!(kind("@set x = 1"), StatementKind::Command);
        // Oracle PL/SQL blocks and calls change what they like.
        let ora = |sql| analyze(sql, Dialect::Oracle).kind;
        assert_eq!(ora("BEGIN DELETE FROM t; END;"), StatementKind::Dml);
        assert_eq!(
            ora("BEGIN EXECUTE IMMEDIATE 'DROP TABLE t'; END;"),
            StatementKind::Dml
        );
        assert_eq!(
            ora("DECLARE x NUMBER; BEGIN NULL; END;"),
            StatementKind::Dml
        );
        assert_eq!(ora("CALL p()"), StatementKind::Dml);
        assert_eq!(ora("EXEC p"), StatementKind::Dml);
        assert_eq!(
            ora("WITH FUNCTION f RETURN NUMBER IS BEGIN RETURN 1; END; SELECT f FROM DUAL"),
            StatementKind::Dml
        );
        assert_eq!(ora("BEGIN"), StatementKind::Other);
        assert_eq!(ora("COMMIT"), StatementKind::Other);
    }

    #[test]
    fn reads_what_the_server_runs() {
        let my = |sql| analyze(sql, Dialect::MySql);
        assert_eq!(my("/*!40101 SET NAMES utf8 */").kind, StatementKind::Other);
        assert_eq!(
            my("SELECT 1 /*! INTO OUTFILE '/tmp/x' */").kind,
            StatementKind::Dml
        );
        assert_eq!(
            changes_in("DELETE FROM t /*! WHERE id = 1 */", Dialect::MySql),
            Vec::<String>::new()
        );
        assert_eq!(
            changes_in("DELETE FROM t /* WHERE id = 1 */", Dialect::MySql),
            ["DELETE t"]
        );
        let ms = |sql| analyze(sql, Dialect::SqlServer).kind;
        assert_eq!(ms("SELECT 1 COMMIT EXEC('x')"), StatementKind::Dml);
        assert_eq!(ms("[xp_cmdshell] 'whoami'"), StatementKind::Dml);
    }
}
