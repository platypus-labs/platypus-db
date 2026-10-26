//! Dialect-aware SQL utilities: statement splitting, the statement under the cursor, a
//! conservative read-only check, the [`Dialect`] rules for generating SQL, the table a query
//! reads ([`source_table`]), the [`dml`] that saves edited table data, the editor's formatter
//! ([`format()`]) and the PostgreSQL array and composite literals the data editor splits
//! ([`pg_literal`]), the editor's parameters ([`parameters`]) and linter ([`lint()`]). All
//! offsets are byte offsets into the input.

mod analysis;
mod dialect;
pub mod dml;
mod format;
mod generate;
mod lexer;
mod lint;
mod params;
pub mod pg_literal;
mod rewrite;
mod source;

pub use analysis::{Analysis, StatementKind, TableName, analyze};
pub use dialect::{Dialect, contains_pattern};
pub use format::{FormatOptions, KeywordCase, format};
pub use generate::ObjectName;
pub use lexer::{Lexer, Token, TokenKind};
pub use lint::{ColumnRef, Lint, LintIssue, LintKind, TableAlias, lint};
pub use params::{Parameter, ParameterKind, parameters};
pub use rewrite::{
    add_condition, count_query, filter_columns, order_by_clause, restore_order_by, set_order_by,
};
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
    /// A Transact-SQL `BEGIN` that opens a block unless `TRANSACTION` follows.
    pending_begin: bool,
    /// A Transact-SQL routine or trigger, which runs to the end of its batch (`GO`), or
    /// Oracle PL/SQL, which runs to a `/` line.
    whole_batch: bool,
    /// The last word was an `END` that closed a block, unless `IF`, `LOOP`… follows.
    ended_block: bool,
}

/// Words between `CREATE` and the kind of object in Oracle.
const ORACLE_CREATE_MODIFIERS: &[&str] = &["OR", "REPLACE", "EDITIONABLE", "NONEDITIONABLE"];

/// Whether an Oracle statement starting with `head` (upper-cased words) is PL/SQL: a block,
/// or a routine, package, trigger or type being created.
fn plsql_head(head: &[String]) -> bool {
    match head {
        [first, ..] if first == "BEGIN" || first == "DECLARE" => true,
        [first, rest @ ..] if first == "CREATE" => rest
            .iter()
            .find(|w| !ORACLE_CREATE_MODIFIERS.contains(&w.as_str()))
            .is_some_and(|kind| {
                matches!(
                    kind.as_str(),
                    "PROCEDURE" | "FUNCTION" | "PACKAGE" | "TRIGGER" | "TYPE"
                )
            }),
        _ => false,
    }
}

/// Whether an Oracle statement is PL/SQL, which keeps its final `;`: the database refuses
/// that `;` after plain SQL.
#[must_use]
pub fn is_plsql(sql: &str) -> bool {
    let head: Vec<String> = Lexer::new(sql, Dialect::Oracle)
        .filter(|t| t.kind == TokenKind::Word)
        .take(ORACLE_CREATE_MODIFIERS.len() + 2)
        .map(|t| t.text(sql).to_ascii_uppercase())
        .collect();
    plsql_head(&head)
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

    /// Whether a `;` here ends the statement.
    fn at_boundary(&self) -> bool {
        self.block_depth == 0 && !self.whole_batch
    }

    fn word(&mut self, word: &str, dialect: Dialect) {
        let upper = word.to_ascii_uppercase();
        if dialect == Dialect::SqlServer {
            self.tsql_word(&upper);
            return;
        }
        if dialect == Dialect::Oracle {
            if self.head.len() < ORACLE_CREATE_MODIFIERS.len() + 2 {
                self.head.push(upper);
                self.whole_batch = self.whole_batch || plsql_head(&self.head);
            }
            return;
        }
        let opens_block = match dialect {
            // SQL-standard function bodies: `BEGIN ATOMIC ...; ...; END`.
            Dialect::Postgres => upper == "ATOMIC" && self.prev_word == "BEGIN",
            // Trigger bodies: `CREATE TRIGGER ... BEGIN ...; END`.
            Dialect::Sqlite => {
                upper == "BEGIN"
                    && self.head.first().is_some_and(|w| w == "CREATE")
                    && self.head.iter().take(3).any(|w| w == "TRIGGER")
            }
            // Compound statements, and the bodies of routines and triggers.
            Dialect::Db2 => upper == "BEGIN" && self.head.first().is_none_or(|w| w == "CREATE"),
            // MySQL bodies rely on `DELIMITER` instead; DuckDB and ClickHouse have no
            // statement bodies; Oracle's run to a `/` line.
            Dialect::MySql
            | Dialect::DuckDb
            | Dialect::SqlServer
            | Dialect::ClickHouse
            | Dialect::Ansi
            | Dialect::Snowflake
            | Dialect::BigQuery
            | Dialect::Databricks
            | Dialect::Oracle => false,
        };
        let after_end = std::mem::take(&mut self.ended_block);
        if opens_block {
            self.block_depth += 1;
        } else if after_end && matches!(upper.as_str(), "IF" | "LOOP" | "WHILE" | "REPEAT" | "FOR")
        {
            // `END IF`, `END LOOP`…: the `END` closed no block.
            self.block_depth += 1;
        } else if self.block_depth > 0 {
            match upper.as_str() {
                // `END CASE` closes the `CASE` statement the `END` counted.
                "CASE" if !after_end => self.block_depth += 1,
                "END" => {
                    self.block_depth -= 1;
                    self.ended_block = true;
                }
                _ => {}
            }
        }
        if self.head.len() < 3 {
            self.head.push(upper.clone());
        }
        self.prev_word = upper;
    }

    /// Transact-SQL blocks: `BEGIN ... END` (also `BEGIN TRY`, `BEGIN CATCH`) but not
    /// `BEGIN TRAN`, and `CREATE [OR ALTER] PROCEDURE | FUNCTION | TRIGGER`, whose body
    /// takes the rest of the batch.
    fn tsql_word(&mut self, upper: &str) {
        if std::mem::take(&mut self.pending_begin)
            && !matches!(
                upper,
                "TRAN" | "TRANSACTION" | "DISTRIBUTED" | "DIALOG" | "CONVERSATION"
            )
        {
            self.block_depth += 1;
        }
        match upper {
            "BEGIN" => self.pending_begin = true,
            "CASE" if self.block_depth > 0 => self.block_depth += 1,
            "END" if self.block_depth > 0 => self.block_depth -= 1,
            _ => {}
        }
        if self.head.len() < 4 {
            self.head.push(upper.to_owned());
            let head: Vec<&str> = self.head.iter().map(String::as_str).collect();
            let routine = |w: &str| matches!(w, "PROC" | "PROCEDURE" | "FUNCTION" | "TRIGGER");
            self.whole_batch = match head.as_slice() {
                ["CREATE" | "ALTER", kind, ..] if routine(kind) => true,
                ["CREATE", "OR", "ALTER", kind] => routine(kind),
                _ => false,
            };
        }
        upper.clone_into(&mut self.prev_word);
    }
}

/// If a Transact-SQL batch separator (`GO`, optionally with a count, alone on its line)
/// is `word`, the end of its line.
fn batch_separator(src: &str, word: &Token) -> Option<usize> {
    if !word.text(src).eq_ignore_ascii_case("go") {
        return None;
    }
    let line_start = src[..word.start].rfind('\n').map_or(0, |i| i + 1);
    if !src[line_start..word.start].trim().is_empty() {
        return None;
    }
    let line_end = src[word.end..]
        .find('\n')
        .map_or(src.len(), |i| word.end + i);
    let rest = src[word.end..line_end].trim();
    let rest = rest.split("--").next().unwrap_or_default().trim();
    rest.bytes().all(|b| b.is_ascii_digit()).then_some(line_end)
}

/// If `token` is an Oracle `/` alone on its line, which ends a statement (as in SQL*Plus),
/// the end of its line.
fn slash_line(src: &str, token: &Token) -> Option<usize> {
    if token.text(src) != "/" {
        return None;
    }
    let line_start = src[..token.start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = src[token.end..]
        .find('\n')
        .map_or(src.len(), |i| token.end + i);
    (src[line_start..token.start].trim().is_empty() && src[token.end..line_end].trim().is_empty())
        .then_some(line_end)
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

/// If a client command (`@set x = 1`, `@include a.sql`) starts at `token`, the end of its
/// text: the rest of the line, without a trailing `;`. No statement starts with `@`.
fn client_command(src: &str, token: &Token) -> Option<usize> {
    if token.kind != TokenKind::Symbol
        || token.text(src) != "@"
        || !src[token.end..].starts_with(|c: char| c.is_ascii_alphabetic())
    {
        return None;
    }
    let line = src[token.start..].lines().next().unwrap_or_default();
    Some(token.start + line.trim_end().trim_end_matches(';').trim_end().len())
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
        if dialect == Dialect::SqlServer
            && token.kind == TokenKind::Word
            && let Some(line_end) = batch_separator(src, &token)
        {
            statements.extend(current.take());
            lexer.seek(line_end);
            continue;
        }
        if dialect == Dialect::Oracle
            && token.kind == TokenKind::Symbol
            && let Some(line_end) = slash_line(src, &token)
        {
            statements.extend(current.take());
            lexer.seek(line_end);
            continue;
        }
        if current.start.is_none()
            && let Some(end) = client_command(src, &token)
        {
            statements.push(Statement {
                start: token.start,
                end,
            });
            lexer.seek(end);
            continue;
        }
        if current.at_boundary()
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
    fn client_commands_end_at_the_line() {
        let src = "@set id = 1;\n@include 'a b.sql'\r\nselect :id;\n@echo it's done";
        assert_eq!(
            texts(src, Dialect::Postgres),
            [
                "@set id = 1",
                "@include 'a b.sql'",
                "select :id",
                "@echo it's done"
            ]
        );
        // MySQL user variables are not commands.
        assert_eq!(
            texts("select\n@x := 1; set @y = 2", Dialect::MySql),
            ["select\n@x := 1", "set @y = 2"]
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
    fn duckdb_has_no_statement_bodies() {
        let src = "CREATE MACRO m(a) AS a || $$;$$; BEGIN; SELECT m('x'); COMMIT";
        assert_eq!(
            texts(src, Dialect::DuckDb),
            [
                "CREATE MACRO m(a) AS a || $$;$$",
                "BEGIN",
                "SELECT m('x')",
                "COMMIT"
            ]
        );
    }

    #[test]
    fn clickhouse_splits_on_semicolons_outside_quotes() {
        let src = "SELECT 'a\\';b'; # c;\nSELECT `x;y` FROM t; SELECT $h$;$h$";
        assert_eq!(
            texts(src, Dialect::ClickHouse),
            ["SELECT 'a\\';b'", "SELECT `x;y` FROM t", "SELECT $h$;$h$"]
        );
    }

    #[test]
    fn sql_server_batches_and_blocks() {
        let ms = Dialect::SqlServer;
        let src = "CREATE PROCEDURE p AS\nBEGIN\n  SELECT 1;\n  SELECT 2;\nEND;\nGO\n\
                   EXEC p;\ngo 2 -- twice\nselect 3";
        assert_eq!(
            texts(src, ms),
            [
                "CREATE PROCEDURE p AS\nBEGIN\n  SELECT 1;\n  SELECT 2;\nEND;",
                "EXEC p",
                "select 3"
            ]
        );
        let tx = "BEGIN TRAN; UPDATE t SET a = 1; COMMIT TRAN";
        assert_eq!(texts(tx, ms).len(), 3, "BEGIN TRAN is no block");
        let block = "IF 1 = 1 BEGIN SELECT CASE WHEN 1 = 1 THEN 1 END; SELECT 2; END; SELECT 3";
        assert_eq!(
            texts(block, ms),
            [
                "IF 1 = 1 BEGIN SELECT CASE WHEN 1 = 1 THEN 1 END; SELECT 2; END",
                "SELECT 3"
            ]
        );
        let try_catch = "BEGIN TRY SELECT 1/0; END TRY BEGIN CATCH SELECT 2; END CATCH; SELECT 3";
        assert_eq!(texts(try_catch, ms).len(), 2);
        let alter = "CREATE OR ALTER FUNCTION f() RETURNS INT AS BEGIN RETURN 1; END; SELECT 1";
        assert_eq!(texts(alter, ms).len(), 1, "a routine takes its whole batch");
        // `GO` within a line, or as a name, separates nothing.
        assert_eq!(texts("SELECT go FROM t; SELECT 1", ms).len(), 2);
        assert_eq!(texts("SELECT 1 go\nSELECT 2", ms).len(), 1);
    }

    #[test]
    fn oracle_plsql_runs_to_a_slash_line() {
        let src = "select 1 from dual;\n\
                   BEGIN\n  update t set a = 1;\n  commit;\nEND;\n/\n\
                   CREATE OR REPLACE PROCEDURE p IS\nBEGIN\n  NULL;\nEND p;\n/\n\
                   select q'[it's; here]' from dual\n/\nselect 2 from dual";
        assert_eq!(
            texts(src, Dialect::Oracle),
            [
                "select 1 from dual",
                "BEGIN\n  update t set a = 1;\n  commit;\nEND;",
                "CREATE OR REPLACE PROCEDURE p IS\nBEGIN\n  NULL;\nEND p;",
                "select q'[it's; here]' from dual",
                "select 2 from dual",
            ]
        );
        // A `/` inside a line divides.
        assert_eq!(texts("select 4 / 2 from dual;", Dialect::Oracle).len(), 1);
        assert!(is_plsql("declare x number; begin null; end;"));
        assert!(is_plsql(
            "create or replace editionable package body k as end;"
        ));
        assert!(is_plsql(
            "CREATE TRIGGER t BEFORE INSERT ON x BEGIN NULL; END;"
        ));
        assert!(!is_plsql("create table t (a number)"));
        assert!(!is_plsql(
            "create or replace view v as select 1 x from dual"
        ));
        assert!(!is_plsql("select 1 from dual"));
    }

    #[test]
    fn db2_compound_bodies() {
        let src = "CREATE PROCEDURE p() BEGIN\n  IF 1 = 1 THEN SET x = CASE WHEN a THEN 1 END; END IF;\n  \
                   CASE y WHEN 1 THEN SET z = 2; END CASE;\nEND;\nselect 1 from sysibm.sysdummy1;\n\
                   BEGIN\n  WHILE i < 3 DO SET i = i + 1; END WHILE;\nEND";
        let parts = texts(src, Dialect::Db2);
        assert_eq!(parts.len(), 3, "{parts:?}");
        assert!(parts[0].ends_with("END CASE;\nEND"), "{parts:?}");
        assert_eq!(parts[1], "select 1 from sysibm.sysdummy1");
        assert!(parts[2].starts_with("BEGIN") && parts[2].ends_with("END"));
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
