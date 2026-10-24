//! Dialect-aware SQL utilities: statement splitting, the statement under the cursor, a
//! conservative read-only check, the [`Dialect`] rules for generating SQL, the table a query
//! reads ([`source_table`]), the [`dml`] that saves edited table data and the editor's
//! formatter ([`format()`]). All offsets are byte offsets into the input.

mod analysis;
mod dialect;
pub mod dml;
mod format;
mod generate;
mod lexer;
mod source;

pub use analysis::{Analysis, StatementKind, TableName, analyze};
pub use dialect::Dialect;
pub use format::{FormatOptions, KeywordCase, format};
pub use generate::ObjectName;
pub use lexer::{Lexer, Token, TokenKind};
pub use source::{SelectItem, SourceTable, source_table};

/// A statement's byte range: from its first token to its last, excluding surrounding
/// comments, whitespace and the delimiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Statement {
    pub start: usize,
    pub end: usize,
}

impl Statement {
    #[must_use]
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }
}

/// Tracks the statement being built while scanning tokens.
#[derive(Default)]
struct Current {
    start: Option<usize>,
    end: usize,
    /// First words, upper-cased, to recognise `CREATE [TEMP] TRIGGER`.
    head: Vec<String>,
    prev_word: String,
    /// Nesting of `BEGIN ... END` bodies whose inner `;` do not end the statement.
    block_depth: usize,
}

impl Current {
    fn take(&mut self) -> Option<Statement> {
        let start = self.start?;
        let statement = Statement {
            start,
            end: self.end,
        };
        *self = Self::default();
        Some(statement)
    }

    fn word(&mut self, word: &str, dialect: Dialect) {
        let upper = word.to_ascii_uppercase();
        let opens_block = match dialect {
            // SQL-standard function bodies: `BEGIN ATOMIC ...; ...; END`.
            Dialect::Postgres => upper == "ATOMIC" && self.prev_word == "BEGIN",
            // Trigger bodies: `CREATE TRIGGER ... BEGIN ...; END`.
            Dialect::Sqlite => {
                upper == "BEGIN"
                    && self.head.first().is_some_and(|w| w == "CREATE")
                    && self.head.iter().take(3).any(|w| w == "TRIGGER")
            }
            // MySQL bodies rely on `DELIMITER` instead.
            Dialect::MySql => false,
        };
        if opens_block {
            self.block_depth += 1;
        } else if self.block_depth > 0 {
            match upper.as_str() {
                "CASE" => self.block_depth += 1,
                "END" => self.block_depth -= 1,
                _ => {}
            }
        }
        if self.head.len() < 3 {
            self.head.push(upper.clone());
        }
        self.prev_word = upper;
    }
}

/// If a MySQL `DELIMITER` client command starts at `pos`, returns the new delimiter and
/// the end of its line.
fn delimiter_command(src: &str, word: &Token) -> Option<(String, usize)> {
    if !word.text(src).eq_ignore_ascii_case("delimiter") {
        return None;
    }
    let line_end = src[word.end..]
        .find('\n')
        .map_or(src.len(), |i| word.end + i);
    let delimiter = src[word.end..line_end].split_whitespace().next()?;
    Some((delimiter.to_owned(), line_end))
}

/// Splits a script into statements. Never fails: unterminated quotes or comments simply
/// extend to the end of the input.
#[must_use]
pub fn split(src: &str, dialect: Dialect) -> Vec<Statement> {
    let mut statements = Vec::new();
    let mut lexer = Lexer::new(src, dialect);
    let mut current = Current::default();
    let mut delimiter = String::from(";");

    while let Some(token) = lexer.next() {
        if token.is_trivia() {
            continue;
        }
        if dialect == Dialect::MySql
            && current.start.is_none()
            && token.kind == TokenKind::Word
            && let Some((new, line_end)) = delimiter_command(src, &token)
        {
            delimiter = new;
            lexer.seek(line_end);
            continue;
        }
        if current.block_depth == 0
            && token.kind == TokenKind::Symbol
            && src[token.start..].starts_with(&delimiter)
        {
            statements.extend(current.take());
            lexer.seek(token.start + delimiter.len());
            continue;
        }
        current.start.get_or_insert(token.start);
        current.end = token.end;
        if token.kind == TokenKind::Word {
            current.word(token.text(src), dialect);
        }
    }
    statements.extend(current.take());
    statements
}

/// The statement to run for a cursor at `offset`: the one containing it, else the closest
/// one before it (the cursor usually sits after a `;` or on the next line), else the next.
#[must_use]
pub fn statement_at(src: &str, dialect: Dialect, offset: usize) -> Option<Statement> {
    let statements = split(src, dialect);
    statements
        .iter()
        .rev()
        .find(|s| s.start <= offset)
        .or_else(|| statements.first())
        .copied()
}

const READ_ONLY_LEADERS: &[&str] = &[
    "SELECT", "WITH", "SHOW", "EXPLAIN", "VALUES", "TABLE", "DESCRIBE", "DESC",
];

/// Words that write anywhere in a statement, including data-modifying CTEs, `SELECT INTO`
/// and `EXPLAIN ANALYZE` of DML.
const WRITE_WORDS: &[&str] = &[
    "INSERT",
    "UPDATE",
    "DELETE",
    "MERGE",
    "UPSERT",
    "REPLACE",
    "CREATE",
    "ALTER",
    "DROP",
    "TRUNCATE",
    "GRANT",
    "REVOKE",
    "COPY",
    "CALL",
    "DO",
    "INTO",
    "LOCK",
    "VACUUM",
    "NEXTVAL",
    "SETVAL",
    "PG_TERMINATE_BACKEND",
    "PG_CANCEL_BACKEND",
];

/// Conservative check used to guard read-only connections: `true` only for statements that
/// start like a query and mention no writing keyword. Functions with side effects called
/// from a `SELECT` cannot be detected; `SELECT ... FOR UPDATE` errs on the safe side.
#[must_use]
pub fn is_read_only(sql: &str, dialect: Dialect) -> bool {
    let mut words = Lexer::new(sql, dialect)
        .filter(|t| t.kind == TokenKind::Word)
        .map(|t| t.text(sql).to_ascii_uppercase());
    let Some(first) = words.next() else {
        return true;
    };
    READ_ONLY_LEADERS.contains(&first.as_str()) && words.all(|w| !WRITE_WORDS.contains(&w.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str, d: Dialect) -> Vec<&str> {
        split(src, d).iter().map(|s| s.text(src)).collect()
    }

    #[test]
    fn splits_simple_scripts() {
        let src = "select 1;\n  select 2 ;\n\n-- trailing comment\nselect 3";
        assert_eq!(
            texts(src, Dialect::Postgres),
            ["select 1", "select 2", "select 3"]
        );
    }

    #[test]
    fn ignores_empty_statements_and_comment_only_parts() {
        let src = ";; -- nothing\n/* still nothing */;\nselect 1;;";
        assert_eq!(texts(src, Dialect::Postgres), ["select 1"]);
        assert_eq!(split("  -- only a comment", Dialect::Postgres), []);
    }

    #[test]
    fn keeps_semicolons_in_quotes_and_comments() {
        let src = "select ';' as a, \"x;y\" /* ; */ -- ;\n from t; select 2";
        assert_eq!(
            texts(src, Dialect::Postgres),
            ["select ';' as a, \"x;y\" /* ; */ -- ;\n from t", "select 2"]
        );
    }

    #[test]
    fn postgres_function_bodies() {
        let src = "create function f() returns int language plpgsql as $$\
                   begin perform 1; return 2; end $$;\nselect f();";
        assert_eq!(split(src, Dialect::Postgres).len(), 2);

        let atomic = "create function g() returns int language sql begin atomic \
                      select case when true then 1 end; select 2; end; select g();";
        assert_eq!(
            texts(atomic, Dialect::Postgres),
            [
                "create function g() returns int language sql begin atomic \
                 select case when true then 1 end; select 2; end",
                "select g()"
            ]
        );
        // A plain transaction BEGIN is not a block.
        assert_eq!(
            texts("begin; select 1; commit;", Dialect::Postgres).len(),
            3
        );
    }

    #[test]
    fn mysql_delimiter_command() {
        let src = "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END//\n\
                   DELIMITER ;\nCALL p();\nselect '//';";
        assert_eq!(
            texts(src, Dialect::MySql),
            [
                "CREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END",
                "CALL p()",
                "select '//'"
            ]
        );
    }

    #[test]
    fn sqlite_trigger_bodies() {
        let src = "CREATE TEMP TRIGGER t AFTER INSERT ON a BEGIN \
                   UPDATE b SET n = CASE WHEN 1 THEN 2 END; DELETE FROM c; END; select 1";
        assert_eq!(split(src, Dialect::Sqlite).len(), 2);
        assert_eq!(texts("BEGIN; select 1; COMMIT", Dialect::Sqlite).len(), 3);
    }

    #[test]
    fn finds_statement_at_cursor() {
        let src = "select 1;\nselect 2;\n\nselect 3";
        let at = |offset| statement_at(src, Dialect::Postgres, offset).map(|s| s.text(src));
        assert_eq!(at(0), Some("select 1"));
        assert_eq!(at(4), Some("select 1"));
        assert_eq!(at(9), Some("select 1"), "right after the semicolon");
        assert_eq!(at(10), Some("select 2"));
        assert_eq!(at(20), Some("select 2"), "blank line after it");
        assert_eq!(at(src.len()), Some("select 3"));
        let leading = "\n\n  select 9";
        assert_eq!(
            statement_at(leading, Dialect::Postgres, 0).map(|s| s.text(leading)),
            Some("select 9")
        );
        assert_eq!(statement_at("", Dialect::Postgres, 0), None);
    }

    #[test]
    fn read_only_check() {
        let ro = |sql| is_read_only(sql, Dialect::Postgres);
        assert!(ro("select * from t"));
        assert!(ro("  -- note\n WITH x AS (select 1) select * from x"));
        assert!(ro("explain select 1"));
        assert!(
            ro("select 'delete' as \"update\""),
            "words in strings and quoted identifiers"
        );
        assert!(!ro("with d as (delete from t returning *) select * from d"));
        assert!(!ro("select * into backup from t"));
        assert!(!ro("explain analyze update t set x = 1"));
        assert!(!ro("select nextval('s')"));
        assert!(!ro("update t set x = 1"));
        assert!(!ro("select * from t for update"), "errs on the safe side");
    }
}
