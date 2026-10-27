//! What a statement does, checked before it runs: the app asks before changes that hit every
//! row of a table, and before any change on a production connection.

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    pub kind: StatementKind,
    /// An `UPDATE` or `DELETE` without `WHERE` (or `LIMIT`), or a `TRUNCATE`: the table
    /// whose every row it changes.
    pub unfiltered: Option<TableName>,
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

pub(crate) struct Words<'a> {
    src: &'a str,
    pub(crate) tokens: Vec<Token>,
}

impl<'a> Words<'a> {
    /// The tokens of `sql` without whitespace and comments, reading the body of a MySQL
    /// executable comment (`/*! … */`) as the code it is.
    pub(crate) fn new(sql: &'a str, dialect: Dialect) -> Self {
        Words {
            src: sql,
            tokens: Lexer::executable(sql, dialect)
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

    /// Whether one of `words` appears after `from`, outside parentheses.
    pub(crate) fn at_top(&self, from: usize, words: &[&str]) -> bool {
        let mut depth = 0usize;
        (from..self.tokens.len()).any(|i| {
            if self.symbol(i, "(") {
                depth += 1;
            } else if self.symbol(i, ")") {
                depth = depth.saturating_sub(1);
            }
            depth == 0 && self.is(i, words)
        })
    }

    /// The first word at top level from `from` that is one of `words`.
    pub(crate) fn find_top(&self, from: usize, words: &[&str]) -> Option<usize> {
        let mut depth = 0usize;
        (from..self.tokens.len()).find(|&i| {
            if self.symbol(i, "(") {
                depth += 1;
            } else if self.symbol(i, ")") {
                depth = depth.saturating_sub(1);
            }
            depth == 0 && self.is(i, words)
        })
    }
}

/// The table a statement at `at` (an `UPDATE`, `DELETE` or `TRUNCATE`) changes in every row.
fn unfiltered(w: &Words<'_>, at: usize, dialect: Dialect) -> Option<TableName> {
    let verb = w.word(at)?;
    let mut i = at + 1;
    while w.is(i, MODIFIERS) {
        i += 1;
    }
    match verb.as_str() {
        "TRUNCATE" => {
            if w.is(i, &["TABLE"]) {
                i += 1;
            }
            while w.is(i, &["ONLY"]) {
                i += 1;
            }
            w.table(i, dialect)
        }
        "UPDATE" => (!w.at_top(i, &["WHERE", "LIMIT"]))
            .then(|| w.table(i, dialect))
            .flatten(),
        "DELETE" => {
            // `DELETE FROM t`, or MySQL's `DELETE t FROM t JOIN …`.
            if w.is(i, &["FROM"]) {
                i += 1;
                while w.is(i, &["ONLY"]) {
                    i += 1;
                }
            }
            (!w.at_top(i, &["WHERE", "LIMIT"]))
                .then(|| w.table(i, dialect))
                .flatten()
        }
        _ => None,
    }
}

#[must_use]
pub fn analyze(sql: &str, dialect: Dialect) -> Analysis {
    if sql.trim_start().starts_with('@') {
        return Analysis {
            kind: StatementKind::Command,
            unfiltered: None,
        };
    }
    let w = Words::new(sql, dialect);
    let first = w.word(0).unwrap_or_default();
    // A `WITH` names its main statement after the common table expressions.
    let main = if first == "WITH" {
        w.find_top(1, &["SELECT", "INSERT", "UPDATE", "DELETE", "MERGE"])
            .unwrap_or(0)
    } else {
        0
    };
    let kind = if is_read_only(sql, dialect) {
        StatementKind::Query
    } else if DDL.contains(&first.as_str()) {
        StatementKind::Ddl
    } else if OTHER.contains(&first.as_str()) {
        StatementKind::Other
    } else {
        StatementKind::Dml
    };
    let unfiltered = match kind {
        StatementKind::Dml | StatementKind::Ddl => unfiltered(&w, main, dialect),
        _ => None,
    };
    Analysis { kind, unfiltered }
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

    fn unfiltered_of(sql: &str) -> Option<TableName> {
        analyze(sql, Dialect::Postgres).unfiltered
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
        let mysql = analyze("DELETE LOW_PRIORITY FROM `My Table`", Dialect::MySql);
        assert_eq!(mysql.unfiltered, Some(table(None, "My Table")));
        let tsql = analyze("DELETE FROM [dbo].[My]]T]", Dialect::SqlServer);
        assert_eq!(tsql.unfiltered, Some(table(Some("dbo"), "My]T")));
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
    }

    #[test]
    fn reads_what_the_server_runs() {
        let my = |sql| analyze(sql, Dialect::MySql);
        assert_eq!(my("/*!40101 SET NAMES utf8 */").kind, StatementKind::Other);
        assert_eq!(
            my("SELECT 1 /*! INTO OUTFILE '/tmp/x' */").kind,
            StatementKind::Dml
        );
        assert_eq!(my("DELETE FROM t /*! WHERE id = 1 */").unfiltered, None);
        assert_eq!(
            my("DELETE FROM t /* WHERE id = 1 */").unfiltered,
            Some(table(None, "t"))
        );
        let ms = |sql| analyze(sql, Dialect::SqlServer).kind;
        assert_eq!(ms("SELECT 1 COMMIT EXEC('x')"), StatementKind::Dml);
        assert_eq!(ms("[xp_cmdshell] 'whoami'"), StatementKind::Dml);
    }
}
