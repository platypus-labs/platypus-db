//! PL/SQL around the statements: the compilation errors of a routine, package, trigger or
//! type created invalid, and the SQL*Plus commands scripts often hold (`EXEC`, `SHOW
//! ERRORS`).

use std::fmt::Write as _;

use platypus_core::{DbError, Error};
use platypus_sql::{Dialect, Lexer, Token, TokenKind};

/// Words between `CREATE` (or `ALTER`) and the kind of object.
const MODIFIERS: &[&str] = &["OR", "REPLACE", "EDITIONABLE", "NONEDITIONABLE"];

/// The kinds of stored PL/SQL, as `ALL_ERRORS.TYPE` names them (with `BODY` when one follows).
const KINDS: &[&str] = &["PROCEDURE", "FUNCTION", "PACKAGE", "TRIGGER", "TYPE"];

/// A routine, package, trigger or type a statement creates or compiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedObject {
    pub owner: Option<String>,
    pub name: String,
    /// `PROCEDURE`, `PACKAGE BODY`…
    pub kind: String,
    /// Where the kind's word starts in the statement: line 1 of the object's source.
    pub at: usize,
}

/// A line of `ALL_ERRORS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub line: i64,
    pub column: i64,
    /// `PLS-00201: identifier 'X' must be declared`.
    pub text: String,
    /// An error, not a warning (`PLW-`).
    pub error: bool,
}

impl CompileError {
    #[must_use]
    pub fn line_text(&self) -> String {
        format!("line {}, column {}: {}", self.line, self.column, self.text)
    }
}

/// A name as Oracle stores it: quoted as written, else upper-cased.
fn name(sql: &str, t: &Token) -> Option<String> {
    match t.kind {
        TokenKind::QuotedIdent => Some(t.text(sql).trim_matches('"').replace("\"\"", "\"")),
        TokenKind::Word => Some(t.text(sql).to_ascii_uppercase()),
        _ => None,
    }
}

/// The object `CREATE [OR REPLACE] PROCEDURE | FUNCTION | PACKAGE [BODY] | TRIGGER | TYPE
/// [BODY] [schema.]name`, or `ALTER … COMPILE`, makes.
#[must_use]
pub fn created_object(sql: &str) -> Option<CreatedObject> {
    let tokens: Vec<Token> = Lexer::new(sql, Dialect::Oracle)
        .filter(|t| !t.is_trivia())
        .collect();
    let word = |i: usize| {
        tokens
            .get(i)
            .filter(|t| t.kind == TokenKind::Word)
            .map(|t| t.text(sql).to_ascii_uppercase())
    };
    if !matches!(word(0).as_deref(), Some("CREATE" | "ALTER")) {
        return None;
    }
    let mut i = 1;
    while word(i).is_some_and(|w| MODIFIERS.contains(&w.as_str())) {
        i += 1;
    }
    let kind_word = word(i).filter(|w| KINDS.contains(&w.as_str()))?;
    // A trigger counts the lines of its block, the others from their kind's word.
    let at = if kind_word == "TRIGGER" {
        (i..tokens.len())
            .find(|&j| matches!(word(j).as_deref(), Some("DECLARE" | "BEGIN" | "COMPOUND")))
            .map_or(tokens[i].start, |j| tokens[j].start)
    } else {
        tokens[i].start
    };
    i += 1;
    let kind = if word(i).as_deref() == Some("BODY") {
        i += 1;
        format!("{kind_word} BODY")
    } else {
        kind_word
    };
    if word(i).as_deref() == Some("IF") {
        i += 3;
    }
    let first = name(sql, tokens.get(i)?)?;
    let dotted = tokens
        .get(i + 1)
        .is_some_and(|t| t.kind == TokenKind::Symbol && t.text(sql) == ".");
    let (owner, name) = match dotted.then(|| tokens.get(i + 2)).flatten() {
        Some(t) => (Some(first), name(sql, t)?),
        None => (None, first),
    };
    Some(CreatedObject {
        owner,
        name,
        kind,
        at,
    })
}

impl CreatedObject {
    /// The errors of the object: line, column, text and `ERROR` or `WARNING`.
    #[must_use]
    pub fn errors_sql(&self) -> String {
        let lit = |s: &str| Dialect::Oracle.quote_literal(s);
        let owner = self.owner.as_deref().map_or_else(
            || "SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA')".to_owned(),
            lit,
        );
        format!(
            "SELECT LINE, POSITION, TEXT, ATTRIBUTE FROM ALL_ERRORS \
             WHERE OWNER = {owner} AND NAME = {} AND TYPE = {} ORDER BY SEQUENCE",
            lit(&self.name),
            lit(&self.kind)
        )
    }

    /// The byte of the statement at `line` and `column` (counted from 1, in characters) of
    /// the object's source, counted from 1.
    #[must_use]
    pub fn position(&self, sql: &str, line: i64, column: i64) -> Option<u32> {
        let source = sql.get(self.at..)?;
        let skip = usize::try_from(line.checked_sub(1)?).ok()?;
        let mut start = 0;
        for _ in 0..skip {
            start += source[start..].find('\n')? + 1;
        }
        let text = &source[start..];
        let chars = usize::try_from(column.checked_sub(1)?).ok()?;
        let offset = text
            .char_indices()
            .nth(chars)
            .map_or(text.len(), |(i, _)| i);
        u32::try_from(self.at + start + offset + 1).ok()
    }
}

/// The error of a statement Oracle ran with compilation errors: the first error, at its
/// position, with all of them in the detail; `None` when it found only warnings.
#[must_use]
pub fn compile_error(
    sql: &str,
    object: Option<&CreatedObject>,
    errors: &[CompileError],
) -> Option<Error> {
    let what = object.map_or_else(
        || "The object".to_owned(),
        |o| format!("{} {}", o.kind, o.name),
    );
    if errors.is_empty() {
        return Some(Error::Database(DbError {
            message: format!("{what} was created with compilation errors"),
            code: Some("ORA-24344".to_owned()),
            ..DbError::default()
        }));
    }
    // The error with a code, rather than its "PL/SQL: Statement ignored".
    let first = errors
        .iter()
        .filter(|e| e.error)
        .find(|e| e.text.starts_with("PLS-") || e.text.starts_with("ORA-"))
        .or_else(|| errors.iter().find(|e| e.error))?;
    let (code, text) = match first.text.split_once(':') {
        Some((code, text)) if code.starts_with("PLS-") || code.starts_with("ORA-") => {
            (Some(code.to_owned()), text.trim().to_owned())
        }
        _ => (None, first.text.clone()),
    };
    Some(Error::Database(DbError {
        message: format!(
            "{text} (line {}, column {}: {what} was created with compilation errors)",
            first.line, first.column
        ),
        code,
        detail: Some(
            errors
                .iter()
                .map(CompileError::line_text)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        hint: None,
        position: object.and_then(|o| o.position(sql, first.line, first.column)),
    }))
}

/// A SQL*Plus command as the statement that does it: `EXEC[UTE] call` runs as a PL/SQL
/// block, `SHOW ERR[ORS] [kind name]` reads the compilation errors. `None` for any other
/// statement.
#[must_use]
pub fn command(sql: &str) -> Option<String> {
    let tokens: Vec<Token> = Lexer::new(sql, Dialect::Oracle)
        .filter(|t| !t.is_trivia())
        .collect();
    let word = |i: usize| {
        tokens
            .get(i)
            .filter(|t| t.kind == TokenKind::Word)
            .map(|t| t.text(sql).to_ascii_uppercase())
    };
    match word(0)?.as_str() {
        "EXEC" | "EXECUTE" if word(1).as_deref() != Some("IMMEDIATE") && tokens.len() > 1 => {
            let call = sql[tokens[1].start..].trim_end();
            let call = call.strip_suffix(';').unwrap_or(call).trim_end();
            Some(format!("BEGIN {call}; END;"))
        }
        "SHOW" if matches!(word(1).as_deref(), Some("ERR" | "ERRORS")) => {
            let mut filter = String::new();
            let mut i = 2;
            if let Some(kind) = word(i).filter(|w| KINDS.contains(&w.as_str())) {
                i += 1;
                let kind = if word(i).as_deref() == Some("BODY") {
                    i += 1;
                    format!("{kind} BODY")
                } else {
                    kind
                };
                filter = format!(" WHERE TYPE = '{kind}'");
            }
            if let Some(object) = tokens.get(i).and_then(|t| name(sql, t)) {
                let object = tokens
                    .get(i + 2)
                    .filter(|_| tokens.get(i + 1).is_some_and(|t| t.text(sql) == "."))
                    .and_then(|t| name(sql, t))
                    .unwrap_or(object);
                let joint = if filter.is_empty() { "WHERE" } else { "AND" };
                let _ = write!(
                    filter,
                    " {joint} NAME = {}",
                    Dialect::Oracle.quote_literal(&object)
                );
            }
            Some(format!(
                "SELECT TYPE, NAME, LINE, POSITION, TEXT FROM USER_ERRORS{filter} \
                 ORDER BY TYPE, NAME, SEQUENCE"
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_object_created() {
        let sql = "CREATE OR REPLACE EDITIONABLE PACKAGE BODY hr.\"Pay\" AS END;";
        assert_eq!(
            created_object(sql),
            Some(CreatedObject {
                owner: Some("HR".to_owned()),
                name: "Pay".to_owned(),
                kind: "PACKAGE BODY".to_owned(),
                at: 30,
            })
        );
        let object = created_object("alter procedure p compile").unwrap();
        assert_eq!(
            (object.kind.as_str(), object.name.as_str()),
            ("PROCEDURE", "P")
        );
        assert_eq!(created_object("CREATE TABLE t (a INT)"), None);
        let trigger = "CREATE TRIGGER t BEFORE INSERT ON x FOR EACH ROW\nBEGIN\n  y := 1;\nEND;";
        let object = created_object(trigger).unwrap();
        let at = usize::try_from(object.position(trigger, 2, 3).unwrap()).unwrap() - 1;
        assert!(trigger[at..].starts_with("y := 1"));
        assert_eq!(created_object("SELECT 1 FROM DUAL"), None);
    }

    #[test]
    fn compile_errors_point_at_their_line() {
        let sql = "CREATE OR REPLACE PROCEDURE p AS\nBEGIN\n  x := 1;\nEND;";
        let object = created_object(sql).unwrap();
        let errors = [
            CompileError {
                line: 3,
                column: 3,
                text: "PLS-00201: identifier 'X' must be declared".to_owned(),
                error: true,
            },
            CompileError {
                line: 3,
                column: 3,
                text: "PL/SQL: Statement ignored".to_owned(),
                error: true,
            },
        ];
        let Some(Error::Database(e)) = compile_error(sql, Some(&object), &errors) else {
            panic!("no error");
        };
        assert_eq!(e.code.as_deref(), Some("PLS-00201"));
        assert!(
            e.message
                .starts_with("identifier 'X' must be declared (line 3"),
            "{}",
            e.message
        );
        let at = usize::try_from(e.position.unwrap()).unwrap() - 1;
        assert!(sql[at..].starts_with("x := 1"));
        assert!(e.detail.unwrap().contains("PL/SQL: Statement ignored"));
        let warning = CompileError {
            error: false,
            ..errors[0].clone()
        };
        assert!(compile_error(sql, Some(&object), &[warning]).is_none());
        assert!(compile_error(sql, None, &[]).is_some());
    }

    #[test]
    fn sql_plus_commands() {
        assert_eq!(
            command("EXEC DBMS_OUTPUT.PUT_LINE('x');").as_deref(),
            Some("BEGIN DBMS_OUTPUT.PUT_LINE('x'); END;")
        );
        assert_eq!(command("execute p").as_deref(), Some("BEGIN p; END;"));
        assert_eq!(command("EXECUTE IMMEDIATE 'x'"), None);
        assert_eq!(
            command("show errors").as_deref(),
            Some(
                "SELECT TYPE, NAME, LINE, POSITION, TEXT FROM USER_ERRORS \
                 ORDER BY TYPE, NAME, SEQUENCE"
            )
        );
        assert!(
            command("SHOW ERR PACKAGE BODY hr.pkg")
                .unwrap()
                .contains("WHERE TYPE = 'PACKAGE BODY' AND NAME = 'PKG'")
        );
        assert_eq!(command("SELECT 1 FROM DUAL"), None);
    }
}
