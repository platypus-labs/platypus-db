//! Dialect-aware SQL utilities: statement splitting, the statement under the cursor, a
//! conservative read-only check, the [`Dialect`] rules for generating SQL, the table a query
//! reads ([`source_table`]), the [`dml`] that saves edited table data and the columns it leaves
//! to the database ([`generated`]), the editor's formatter
//! ([`format()`]) and the PostgreSQL array and composite literals the data editor splits
//! ([`pg_literal`]), the editor's parameters ([`parameters`]) and linter ([`lint()`]). All
//! offsets are byte offsets into the input.

mod analysis;
mod dialect;
pub mod dml;
mod format;
mod generate;
pub mod generated;
mod lexer;
mod lint;
mod params;
pub mod pg_literal;
mod rewrite;
pub mod snapshot;
mod source;

pub use analysis::{Analysis, StatementKind, TableName, Unfiltered, analyze};
pub use dialect::{Dialect, contains_pattern};
pub use format::{FormatOptions, KeywordCase, format};
pub use generate::ObjectName;
pub use lexer::{Lexer, Token, TokenKind};
pub use lint::{ColumnRef, Lint, LintIssue, LintKind, TableAlias, lint};
pub use params::{Parameter, ParameterKind, parameters};
pub use rewrite::{
    FilterColumn, add_condition, add_limit, count_query, filter_columns, order_by_clause,
    restore_order_by, set_order_by,
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

/// Whether an Oracle statement starting with `head` (upper-cased words) runs to a `/` line,
/// as its `;` do not end it: PL/SQL ([`plsql_head`]), or a query that declares PL/SQL
/// functions first (`WITH FUNCTION f … END; SELECT f(1) FROM DUAL`, 12c).
fn oracle_block_head(head: &[String]) -> bool {
    plsql_head(head)
        || matches!(head, [with, routine, third, ..]
            if with == "WITH" && (routine == "FUNCTION" || routine == "PROCEDURE") && third != "AS")
}

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
                self.whole_batch = self.whole_batch || oracle_block_head(&self.head);
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
/// is `word`, the end of its line and how many times the batch before it runs (`GO 3`).
fn batch_separator(src: &str, word: &Token) -> Option<(usize, u32)> {
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
    if !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // `GO` alone runs the batch once, and so does a count too large to be meant.
    let count = rest.parse::<u32>().map_or(1, |n| n.max(1));
    Some((line_end, count))
}

/// If the custom delimiter `delimiter` (MySQL `DELIMITER $$`) ends within the word
/// `token`, where it is glued to the last word of a statement (`END$$`: `$` belongs to MySQL
/// names), the offset where it starts. The `mysql` client finds it anywhere outside quotes.
fn delimiter_in_word(src: &str, token: &Token, delimiter: &str) -> Option<usize> {
    if delimiter == ";" || token.kind != TokenKind::Word {
        return None;
    }
    token
        .text(src)
        .find(delimiter)
        .filter(|&i| i > 0)
        .map(|i| token.start + i)
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
/// extend to the end of the input. The body of a MySQL executable comment (`/*! … */`) is
/// code, as the server runs it: a `;` there ends a statement, and a dump's
/// `/*!40101 SET … */;` is a statement of its own.
#[must_use]
pub fn split(src: &str, dialect: Dialect) -> Vec<Statement> {
    scan(src, dialect).statements
}

/// A Transact-SQL batch: the statements up to a `GO` line, which the server runs together
/// (a variable declared in one statement lives to the end of the batch), `repeat` times
/// (`GO 3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    /// Indices into [`split`]'s statements.
    pub statements: std::ops::Range<usize>,
    pub repeat: u32,
}

/// The batches of a script: in Transact-SQL the statements between `GO` lines, a client
/// command (`@set`) being a batch of its own; in other dialects each statement.
#[must_use]
pub fn batches(src: &str, dialect: Dialect) -> Vec<Batch> {
    let scan = scan(src, dialect);
    if dialect != Dialect::SqlServer {
        return (0..scan.statements.len())
            .map(|i| Batch {
                statements: i..i + 1,
                repeat: 1,
            })
            .collect();
    }
    let mut out = Vec::new();
    let mut start = 0;
    let mut ends = scan.batch_ends.iter().peekable();
    for (i, s) in scan.statements.iter().enumerate() {
        let command = s.text(src).starts_with('@');
        if command && start < i {
            out.push(Batch {
                statements: start..i,
                repeat: 1,
            });
            start = i;
        }
        // A `GO` right after this statement ends its batch; one with no statement before it
        // since the last ran nothing.
        let mut repeat = None;
        while let Some(&&(after, count)) = ends.peek() {
            if after > i + 1 {
                break;
            }
            ends.next();
            if after == i + 1 && repeat.is_none() {
                repeat = Some(count);
            }
        }
        if command || repeat.is_some() {
            out.push(Batch {
                statements: start..i + 1,
                repeat: if command { 1 } else { repeat.unwrap_or(1) },
            });
            start = i + 1;
        }
    }
    if start < scan.statements.len() {
        out.push(Batch {
            statements: start..scan.statements.len(),
            repeat: 1,
        });
    }
    out
}

/// What [`scan`] found.
struct Scan {
    statements: Vec<Statement>,
    /// Transact-SQL `GO` lines: how many statements precede each, and its count.
    batch_ends: Vec<(usize, u32)>,
}

fn scan(src: &str, dialect: Dialect) -> Scan {
    let mut statements = Vec::new();
    let mut batch_ends = Vec::new();
    let mut lexer = Lexer::executable(src, dialect);
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
            && let Some((line_end, count)) = batch_separator(src, &token)
        {
            statements.extend(current.take());
            batch_ends.push((statements.len(), count));
            lexer.seek(line_end);
            continue;
        }
        if dialect == Dialect::MySql
            && current.at_boundary()
            && let Some(cut) = delimiter_in_word(src, &token, &delimiter)
        {
            current.start.get_or_insert(token.start);
            current.end = cut;
            current.word(&src[token.start..cut], dialect);
            statements.extend(current.take());
            lexer.seek(cut + delimiter.len());
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
    Scan {
        statements,
        batch_ends,
    }
}

/// The statement to run for a cursor at `offset`: the one containing it, else the closest
/// one before it (the cursor usually sits after a `;` or on the next line), else the next.
/// A comment on its own line right above a statement (no blank line between) belongs to that
/// statement.
#[must_use]
pub fn statement_at(src: &str, dialect: Dialect, offset: usize) -> Option<Statement> {
    let statements = split(src, dialect);
    let before = statements.iter().rposition(|s| s.start <= offset);
    let Some(i) = before else {
        return statements.first().copied();
    };
    let current = statements[i];
    if let Some(next) = statements.get(i + 1)
        && offset > current.end
        && comment_above(src, current.end, offset, next.start)
    {
        return Some(*next);
    }
    Some(current)
}

/// Whether the cursor at `offset`, between a statement that ends at `end` and the next one
/// at `next`, stands on a comment line that leads to the next statement: a line of its own
/// after the line where the statement ended, with text on it, and no blank line down to the
/// next statement.
fn comment_above(src: &str, end: usize, offset: usize, next: usize) -> bool {
    let offset = (offset..=next)
        .find(|&i| src.is_char_boundary(i))
        .unwrap_or(next);
    let line_start = src[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line_end = src[offset..].find('\n').map_or(src.len(), |i| offset + i);
    let line = src[line_start..line_end].trim_start();
    if line_start <= end || !["--", "/*", "#", "//"].iter().any(|c| line.starts_with(c)) {
        return false;
    }
    let next_line = src[..next].rfind('\n').map_or(0, |i| i + 1);
    !src[line_start..next_line.max(line_start)]
        .lines()
        .any(|line| line.trim().is_empty())
}

const READ_ONLY_LEADERS: &[&str] = &[
    "SELECT", "WITH", "SHOW", "EXPLAIN", "VALUES", "TABLE", "DESCRIBE", "DESC",
];

/// Words that write anywhere in a statement, including data-modifying CTEs, `SELECT INTO`
/// (and MySQL's `INTO OUTFILE`) and `EXPLAIN ANALYZE` of DML.
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

/// Transact-SQL needs no `;` between the statements of a batch, so `SELECT 1 COMMIT EXEC(…)`
/// is three statements: a query may hold none of the words that start one, nor those that
/// run dynamic SQL or reach outside the database (`OPENROWSET`, `OPENQUERY`). All are
/// reserved, so no bare name is one, except `SEND`, `RECEIVE` and `CONVERSATION` (Service
/// Broker), which a query then has to quote.
const SQL_SERVER_WRITE_WORDS: &[&str] = &[
    "EXEC",
    "EXECUTE",
    "SP_EXECUTESQL",
    "COMMIT",
    "ROLLBACK",
    "SAVE",
    "BEGIN",
    "TRAN",
    "TRANSACTION",
    "DECLARE",
    "SET",
    "USE",
    "IF",
    "WHILE",
    "GOTO",
    "RETURN",
    "BREAK",
    "CONTINUE",
    "PRINT",
    "RAISERROR",
    "WAITFOR",
    "KILL",
    "SHUTDOWN",
    "BACKUP",
    "RESTORE",
    "DBCC",
    "RECONFIGURE",
    "CHECKPOINT",
    "BULK",
    "OPEN",
    "CLOSE",
    "DEALLOCATE",
    "WRITETEXT",
    "UPDATETEXT",
    "SETUSER",
    "REVERT",
    "DENY",
    "DUMP",
    "LOAD",
    "TRIGGER",
    "OPENROWSET",
    "OPENDATASOURCE",
    "OPENQUERY",
    "SEND",
    "RECEIVE",
    "CONVERSATION",
];

/// Oracle runs the PL/SQL a query declares in `WITH FUNCTION`, which may commit an
/// autonomous transaction or run any statement with `EXECUTE IMMEDIATE`.
const ORACLE_WRITE_WORDS: &[&str] = &[
    "FUNCTION",
    "PROCEDURE",
    "PRAGMA",
    "AUTONOMOUS_TRANSACTION",
    "BEGIN",
    "DECLARE",
    "EXECUTE",
    "IMMEDIATE",
    "COMMIT",
    "ROLLBACK",
];

/// Whether `word` (upper case) writes in `dialect` wherever it stands.
fn writes(word: &str, dialect: Dialect) -> bool {
    WRITE_WORDS.contains(&word)
        || match dialect {
            Dialect::SqlServer => SQL_SERVER_WRITE_WORDS.contains(&word) || word.starts_with("XP_"),
            Dialect::Oracle => ORACLE_WRITE_WORDS.contains(&word),
            _ => false,
        }
}

/// The names a server may read in the word `t`: the word itself and, right after a digit,
/// what follows a number that takes letters from it. Transact-SQL reads `0xINSERT` as
/// `0x INSERT` and `1e1EXEC` as `1e1 EXEC`, where the lexer sees one word after a digit.
fn word_readings(src: &str, t: &Token) -> Vec<String> {
    let word = t.text(src);
    let before = &src[..t.start];
    let after_digit = |s: &str| s.ends_with(|c: char| c.is_ascii_digit());
    let glued = after_digit(before) || before.strip_suffix('.').is_some_and(after_digit);
    let mut names = vec![word.to_ascii_uppercase()];
    if glued {
        for (i, c) in word.char_indices() {
            if !(c.is_ascii_hexdigit() || c == 'x' || c == 'X') {
                break;
            }
            names.push(word[i + 1..].to_ascii_uppercase());
        }
    }
    names
}

/// Whether one statement's tokens (no trivia, no executable comment marks) only read.
fn reads_only(src: &str, tokens: &[Token], dialect: Dialect) -> bool {
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
    let Some(first) = tokens
        .iter()
        .position(|t| !(t.kind == TokenKind::Symbol && t.text(src) == "("))
    else {
        // Nothing but parentheses (or nothing at all) runs nothing.
        return true;
    };
    if !word(first).is_some_and(|w| READ_ONLY_LEADERS.contains(&w.as_str())) {
        return false;
    }
    (first..tokens.len()).all(|i| {
        let Some(this) = word(i) else {
            return true;
        };
        let next = word(i + 1);
        let exempt = match this.as_str() {
            // The `replace()` function, not MySQL's or SQLite's `REPLACE` statement.
            "REPLACE" => symbol(i + 1, "("),
            // Transact-SQL query hints: `OPTION (USE HINT (…))`, `OPTION (USE PLAN N'…')`.
            "USE" => matches!(next.as_deref(), Some("HINT" | "PLAN")),
            _ => false,
        };
        // `NEXT VALUE FOR seq` takes a sequence value; `FOR [KEY] SHARE` locks rows.
        let sequence = this == "NEXT"
            && next.as_deref() == Some("VALUE")
            && word(i + 2).as_deref() == Some("FOR");
        let locks =
            this == "SHARE" && i > 0 && matches!(word(i - 1).as_deref(), Some("FOR" | "KEY"));
        let written = word_readings(src, &tokens[i])
            .iter()
            .any(|name| writes(name, dialect));
        !sequence && !locks && (exempt || !written)
    })
}

/// Conservative check used to guard read-only connections: `true` only when every
/// statement in `sql` starts like a query and mentions no writing keyword, however the
/// server reads it ([`Lexer::readings`]): the bodies of MySQL executable comments
/// (`/*! … */`) count as code, every `;` ends a statement (whatever the client's
/// `DELIMITER`), and in Transact-SQL, whose batches need no `;`, so do the words that start
/// another statement (`EXEC`, `COMMIT`, `SET`…). Anything else (a leading quoted name, an
/// unknown first word) is not read-only.
///
/// Functions with side effects called from a `SELECT` cannot be detected (a PostgreSQL
/// function that writes, a volatile one under `EXPLAIN ANALYZE`): a read-only transaction
/// has to stop those. `SELECT ... FOR UPDATE` and `FOR SHARE` err on the safe side.
#[must_use]
pub fn is_read_only(sql: &str, dialect: Dialect) -> bool {
    Lexer::readings(sql, dialect).into_iter().all(|lexer| {
        let tokens: Vec<Token> = lexer
            .filter(|t| !t.is_trivia() && t.kind != TokenKind::ExecComment)
            .collect();
        tokens
            .split(|t| t.kind == TokenKind::Symbol && t.text(sql) == ";")
            .all(|statement| reads_only(sql, statement, dialect))
    })
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
    fn mysql_delimiter_glued_to_the_last_word() {
        let src = "DROP PROCEDURE IF EXISTS p;\nDELIMITER $$\nCREATE PROCEDURE p()\nBEGIN\n  \
                   SELECT 1 AS one;\nEND$$\nDELIMITER ;\nCALL p();";
        assert_eq!(
            texts(src, Dialect::MySql),
            [
                "DROP PROCEDURE IF EXISTS p",
                "CREATE PROCEDURE p()\nBEGIN\n  SELECT 1 AS one;\nEND",
                "CALL p()"
            ]
        );
        // Wherever it stands outside quotes, as the `mysql` client reads it.
        let src = "DELIMITER $$\nSELECT 'a$$b' AS x, a$$SELECT 2$$";
        assert_eq!(
            texts(src, Dialect::MySql),
            ["SELECT 'a$$b' AS x, a", "SELECT 2"]
        );
        // With the default delimiter, `$` stays part of names.
        assert_eq!(
            texts("SELECT a$$b FROM t", Dialect::MySql),
            ["SELECT a$$b FROM t"]
        );
    }

    #[test]
    fn sql_server_scripts_run_by_batch() {
        let ms = Dialect::SqlServer;
        let src = "DECLARE @x int = 5;\nSELECT @x * 2;\nGO\nINSERT INTO t VALUES (1)\nGO 3\n\
                   @set y = 1\nSELECT 1;\nSELECT 2";
        assert_eq!(split(src, ms).len(), 6);
        assert_eq!(
            batches(src, ms),
            [
                Batch {
                    statements: 0..2,
                    repeat: 1
                },
                Batch {
                    statements: 2..3,
                    repeat: 3
                },
                Batch {
                    statements: 3..4,
                    repeat: 1
                },
                Batch {
                    statements: 4..6,
                    repeat: 1
                },
            ]
        );
        // `GO` lines with nothing before them run nothing; `GO 0` runs once.
        let empty = "GO 5\nGO\nSELECT 1\nGO 0\nGO 2";
        assert_eq!(
            batches(empty, ms),
            [Batch {
                statements: 0..1,
                repeat: 1
            }]
        );
        // Elsewhere every statement is a batch of its own.
        assert_eq!(batches("select 1; select 2", Dialect::Postgres).len(), 2);
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
    fn oracle_with_function_runs_to_a_slash_line() {
        let query = "WITH FUNCTION f(x NUMBER) RETURN NUMBER IS BEGIN RETURN x * 2; END;\n\
                     SELECT f(21) v FROM DUAL";
        assert_eq!(texts(query, Dialect::Oracle), [query]);
        let script = format!("{query}\n/\nselect 1 from dual;");
        assert_eq!(
            texts(&script, Dialect::Oracle),
            [query, "select 1 from dual"]
        );
        // Its final `;` goes, as after any query.
        assert!(!is_plsql(query));
        // A common table expression named `function`.
        assert_eq!(
            texts("with function as (select 1 x from dual) select x from function; select 2 from dual", Dialect::Oracle).len(),
            2
        );
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
    fn a_comment_above_a_statement_belongs_to_it() {
        let src = "select 1;\n-- about 2\n-- more\nselect 2; -- after 2\n\n-- loose\n\nselect 3";
        let at = |needle: &str| {
            let offset = src.find(needle).unwrap() + 2;
            statement_at(src, Dialect::Postgres, offset).map(|s| s.text(src))
        };
        assert_eq!(at("-- about"), Some("select 2"));
        assert_eq!(at("-- more"), Some("select 2"));
        assert_eq!(at("-- after"), Some("select 2"), "same line as statement 2");
        assert_eq!(
            at("-- loose"),
            Some("select 2"),
            "a blank line before statement 3"
        );
        let block = "select 1;\n/* x */ select 2";
        let offset = block.find("x */").unwrap();
        assert_eq!(
            statement_at(block, Dialect::Postgres, offset).map(|s| s.text(block)),
            Some("select 2")
        );
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

    /// Statements an attacker may disguise as a query, per dialect: none is read-only.
    mod bypass {
        use crate::Dialect::{self, *};

        pub(super) const ATTEMPTS: &[(Dialect, &str)] = &[
            // MySQL and MariaDB executable comments run their body.
            (MySql, "SELECT 1 /*! INTO OUTFILE '/tmp/x' */"),
            (MySql, "SELECT 1 /*!50000 INTO OUTFILE '/tmp/x' */"),
            (MySql, "sElEcT 1 /*!80000 iNtO dUmPfIlE '/tmp/x' */"),
            (MySql, "SELECT 1 /*M!100100 INTO OUTFILE '/tmp/x' */"),
            (MySql, "SELECT 1 /*!; COMMIT; DELETE FROM t */"),
            (MySql, "SELECT 1 /*!; SET GLOBAL general_log = 1 */"),
            (MySql, "/*!50000 DELETE FROM t */"),
            // MySQL does not nest comments: the first `*/` ends this one.
            (MySql, "SELECT 1 /* /* */ INTO OUTFILE '/tmp/x' -- */"),
            // Under NO_BACKSLASH_ESCAPES, `'a\'` is a whole string.
            (MySql, r"SELECT 'a\'; SET GLOBAL general_log = 1; -- '"),
            (MySql, r"SELECT 'a\' INTO OUTFILE '/tmp/x' -- '"),
            // Under ANSI_QUOTES, `"a\"` is a whole name.
            (MySql, r#"SELECT "a\" INTO OUTFILE '/tmp/x' -- ""#),
            // `--` before a no-break space starts no MySQL comment.
            (
                MySql,
                "SELECT 1 FROM t WHERE 1 --\u{a0} INTO OUTFILE '/tmp/x'",
            ),
            // The server splits at `;` whatever the client's DELIMITER.
            (MySql, "SELECT 1; SET GLOBAL general_log = 1"),
            (MySql, "SELECT * FROM t LOCK IN SHARE MODE"),
            (MySql, "SELECT * FROM t FOR SHARE"),
            (MySql, "SELECT NEXT VALUE FOR s"),
            (MySql, "SELECT NEXTVAL(s)"),
            (MySql, "REPLACE INTO t VALUES (1)"),
            (MySql, "DO SLEEP(1)"),
            (MySql, "HANDLER t OPEN"),
            (MySql, "LOAD DATA INFILE '/etc/passwd' INTO TABLE t"),
            // Transact-SQL batches need no `;` between statements.
            (
                SqlServer,
                "SELECT 1 COMMIT EXEC('INS'+'ERT INTO t VALUES (1)')",
            ),
            (SqlServer, "SELECT 1 EXEC xp_cmdshell 'whoami'"),
            (SqlServer, "SELECT 1 EXECUTE sp_executesql N'DELETE FROM t'"),
            (SqlServer, "select 1 exec master..xp_dirtree 'c:\\'"),
            (SqlServer, "SELECT 1 BEGIN TRAN"),
            (SqlServer, "SELECT 1 ROLLBACK"),
            (SqlServer, "SELECT 1 SAVE TRANSACTION x"),
            (SqlServer, "SELECT 1 DECLARE @s nvarchar(9) = N'x'"),
            (SqlServer, "SELECT 1 SET IMPLICIT_TRANSACTIONS OFF"),
            (SqlServer, "SELECT 1 USE master"),
            (SqlServer, "SELECT 1 WAITFOR DELAY '00:10'"),
            (SqlServer, "SELECT 1 KILL 52"),
            (SqlServer, "SELECT 1 SHUTDOWN WITH NOWAIT"),
            (SqlServer, "SELECT 1 DBCC DROPCLEANBUFFERS"),
            (SqlServer, "SELECT 1 RECONFIGURE"),
            (
                SqlServer,
                "SELECT 1 BACKUP DATABASE d TO DISK = 'c:\\x.bak'",
            ),
            (SqlServer, "SELECT 1 x DISABLE TRIGGER t ON u"),
            (SqlServer, "SELECT 1 RECEIVE * FROM q"),
            (SqlServer, "SELECT 1 IF 1 = 1 DELETE FROM t"),
            (
                SqlServer,
                "WITH x AS (SELECT 1 a) SELECT * FROM x EXEC('x')",
            ),
            (
                SqlServer,
                "SELECT * FROM OPENROWSET(BULK 'c:\\secret.txt', SINGLE_CLOB) x",
            ),
            (SqlServer, "SELECT * FROM OPENQUERY(srv, 'DELETE FROM t')"),
            (
                SqlServer,
                "SELECT * FROM OPENDATASOURCE('SQLNCLI', 'x').d.s.t",
            ),
            (SqlServer, "SELECT a INTO t2 FROM t"),
            (SqlServer, "SELECT NEXT VALUE FOR s"),
            // A procedure called by name at the start of a batch.
            (SqlServer, "[xp_cmdshell] 'whoami'"),
            (SqlServer, "[master]..[xp_cmdshell] 'whoami'"),
            (SqlServer, "\"sp_executesql\" N'DELETE FROM t'"),
            (SqlServer, "sp_executesql N'DELETE FROM t'"),
            // Numbers that end where a keyword starts.
            (SqlServer, "SELECT 0xINSERT INTO t VALUES (1)"),
            (SqlServer, "SELECT 1e1EXEC('x')"),
            (SqlServer, "SELECT 1.e1EXEC('x')"),
            (SqlServer, "SELECT 1\u{200b}EXEC('x')"),
            (SqlServer, "SELECT 1\nGO\nEXEC p"),
            (SqlServer, "SELECT 1 -- x\rEXEC p"),
            // PostgreSQL.
            (
                Postgres,
                "with d as (delete from t returning *) select * from d",
            ),
            (Postgres, "explain analyze delete from t"),
            (Postgres, "explain (analyze) insert into t values (1)"),
            (Postgres, "select 1 into t2"),
            (Postgres, "select * into temp t2 from t"),
            // PostgreSQL ends a `--` comment at a carriage return too.
            (Postgres, "select 1 -- x\r; delete from t"),
            // Under standard_conforming_strings = off, `'x\''` is a whole string.
            (Postgres, r"select 'x\'' into t --'"),
            (Postgres, r"select 'x\''; delete from t; --'"),
            (Postgres, "copy (select 1) to program 'id'"),
            (Postgres, "do $$ begin delete from t; end $$"),
            (Postgres, "select * from t for share"),
            (Postgres, "select * from t for key share"),
            (Postgres, "select * from t for no key update"),
            (Postgres, "call p()"),
            (Postgres, "select setval('s', 1)"),
            (Postgres, "select 1; delete from t"),
            (Postgres, "SELECT\u{3000}1\u{a0}INTO t"),
            (Postgres, "prepare x as delete from t"),
            (Postgres, "execute x"),
            (Postgres, "listen c"),
            (Postgres, "'select'"),
            // SQLite.
            (Sqlite, "PRAGMA query_only = OFF"),
            (Sqlite, "ATTACH 'x.db' AS x"),
            (Sqlite, "select 1; attach 'x.db' as x"),
            (Sqlite, "replace into t values (1)"),
            (Sqlite, "SELECT 1 /* x */; VACUUM INTO '/tmp/x'"),
            (Sqlite, "WITH x AS (SELECT 1) INSERT INTO t SELECT * FROM x"),
            // DuckDB.
            (DuckDb, "COPY t TO '/tmp/x.csv'"),
            (DuckDb, "INSTALL httpfs"),
            (DuckDb, "LOAD httpfs"),
            (DuckDb, "ATTACH 'x.db'"),
            (DuckDb, "select 1; copy t to 'x.csv'"),
            (DuckDb, "SET enable_external_access = true"),
            (DuckDb, "EXPORT DATABASE '/tmp/x'"),
            (DuckDb, "PRAGMA enable_profiling"),
            // Oracle runs the PL/SQL of `WITH FUNCTION`.
            (
                Oracle,
                "WITH FUNCTION f RETURN NUMBER IS PRAGMA AUTONOMOUS_TRANSACTION; BEGIN \
             EXECUTE IMMEDIATE 'DELETE FROM t'; COMMIT; RETURN 1; END; SELECT f FROM dual",
            ),
            (Oracle, "BEGIN DELETE FROM t; END;"),
            (Oracle, "DECLARE x NUMBER; BEGIN NULL; END;"),
            (Oracle, "select s.nextval from dual"),
            (Oracle, "select * from t for update"),
            (Oracle, "EXECUTE IMMEDIATE 'DROP TABLE t'"),
            // The rest have no read-only transaction: the check is all there is.
            (ClickHouse, "SELECT 1; DROP TABLE t"),
            (ClickHouse, "SYSTEM SHUTDOWN"),
            (ClickHouse, "KILL QUERY WHERE 1"),
            (Snowflake, r"SELECT 'a\'' ; DROP TABLE t; --'"),
            (Snowflake, "EXECUTE IMMEDIATE 'DROP TABLE t'"),
            (Snowflake, "CALL p()"),
            (BigQuery, "EXECUTE IMMEDIATE 'DROP TABLE d.t'"),
            (BigQuery, "SELECT 1; DELETE FROM d.t WHERE true"),
            (Databricks, "SELECT 1; DROP TABLE t"),
            (Ansi, "call system.runtime.kill_query('q')"),
            (Db2, "SELECT * FROM FINAL TABLE (INSERT INTO t VALUES (1))"),
            (Db2, "SELECT NEXT VALUE FOR s FROM sysibm.sysdummy1"),
        ];
    }

    #[test]
    fn read_only_bypass_attempts() {
        let attempts = bypass::ATTEMPTS;
        for &(dialect, sql) in attempts {
            assert!(!is_read_only(sql, dialect), "{dialect:?}: {sql}");
        }
    }

    /// Queries that look like the attempts above but only read.
    #[test]
    fn read_only_keeps_plain_queries() {
        use Dialect::*;
        let queries: &[(Dialect, &str)] = &[
            (MySql, "SELECT /*+ MAX_EXECUTION_TIME(1000) */ * FROM t"),
            (MySql, "SELECT /*! STRAIGHT_JOIN */ a FROM t"),
            (
                MySql,
                r"SELECT 'a\'b', REPLACE(name, 'a', 'b') FROM t -- note",
            ),
            (MySql, "SELECT CAST(a AS CHAR CHARACTER SET utf8mb4) FROM t"),
            (MySql, "SELECT '/*! INTO OUTFILE */' FROM t"),
            (
                SqlServer,
                "SELECT CASE WHEN a = 1 THEN 'x' END AS c FROM t WITH (NOLOCK) \
                 ORDER BY a OFFSET 0 ROWS FETCH NEXT 10 ROWS ONLY",
            ),
            (
                SqlServer,
                "SELECT a FROM t OPTION (USE HINT ('DISABLE_OPTIMIZER_ROWGOAL'))",
            ),
            (
                SqlServer,
                "SELECT 0x1F, 1e5, [exec], N'EXEC xp_cmdshell' FROM t;",
            ),
            (SqlServer, "WITH x AS (SELECT 1 a) SELECT * FROM x"),
            (Postgres, "select replace(a, 'x', 'y') from t"),
            (
                Postgres,
                "select $$;delete$$, 'it''s; delete', e'\\';delete'",
            ),
            (Postgres, "/* /* */ delete from t */ select 1"),
            (Postgres, "select \"delete\" from t -- ; delete\r\n"),
            (Postgres, "(select 1) union (select 2)"),
            (Postgres, "table t"),
            (Postgres, "values (1)"),
            (Postgres, "show search_path"),
            (Postgres, "select 1; select 2;"),
            (Oracle, "select q'[delete]' from dual"),
            (DuckDb, "SELECT * FROM t"),
            (Sqlite, "select * from t"),
            (Db2, "SELECT * FROM t FETCH FIRST 10 ROWS ONLY"),
            (Postgres, ""),
        ];
        for &(dialect, sql) in queries {
            assert!(is_read_only(sql, dialect), "{dialect:?}: {sql}");
        }
    }

    #[test]
    fn mysql_executable_comments_are_code() {
        let my = Dialect::MySql;
        assert_eq!(
            texts("SELECT 1 /*!; DELETE FROM t */", my),
            ["SELECT 1 /*!", "DELETE FROM t */"]
        );
        let dump = "/*!40101 SET NAMES utf8mb4 */;\n/*!40014 SET FOREIGN_KEY_CHECKS=0 */;\n\
                    SELECT /*! SQL_NO_CACHE */ 1 /* plain */;";
        assert_eq!(
            texts(dump, my),
            [
                "/*!40101 SET NAMES utf8mb4 */",
                "/*!40014 SET FOREIGN_KEY_CHECKS=0 */",
                "SELECT /*! SQL_NO_CACHE */ 1"
            ]
        );
        // Optimizer hints and plain comments stay comments; other dialects have none.
        assert_eq!(texts("/*+ hint */ /* x */", my), Vec::<&str>::new());
        assert_eq!(
            texts("SELECT 1 /*!; DELETE FROM t */", Dialect::Postgres).len(),
            1
        );
    }
}
