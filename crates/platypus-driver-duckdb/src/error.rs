//! DuckDB errors as [`DbError`]s.
//!
//! DuckDB reports errors as text: `Parser Error: syntax error at or near "x"`, then lines
//! of hints, then the line of the statement with a caret under the culprit:
//!
//! ```text
//! Catalog Error: Table with name nope does not exist!
//! Did you mean "pg_enum"?
//!
//! LINE 1: select * from nope
//!                       ^
//! ```
//!
//! The kind (`Parser`, `Catalog`, ...) becomes the code, the first line the message, the
//! lines after it the hint, and the caret the position.

use platypus_core::{DbError, Error};

/// The kind of error raised by `interrupt`.
const INTERRUPT: &str = "INTERRUPT";

pub fn map_error(e: &duckdb::Error, sql: &str) -> Error {
    match e {
        duckdb::Error::DuckDBFailure(_, Some(text)) => from_text(text, sql),
        other => Error::Other(other.to_string()),
    }
}

fn from_text(text: &str, sql: &str) -> Error {
    let (body, context) = match text.find("\n\nLINE ") {
        Some(i) => (&text[..i], Some(&text[i + 2..])),
        None => (text, None),
    };
    let (code, body) = match body.split_once(" Error: ") {
        Some((kind, rest)) if !kind.contains('\n') => (Some(kind), rest),
        _ => (None, body),
    };
    if code == Some(INTERRUPT) {
        return Error::Cancelled;
    }
    let (message, hint) = match body.split_once('\n') {
        Some((first, rest)) if !rest.trim().is_empty() => (first, Some(rest.trim().to_owned())),
        _ => (body, None),
    };
    Error::Database(DbError {
        code: code.map(str::to_owned),
        message: message.to_owned(),
        hint,
        position: context.and_then(|c| position(c, sql)),
        ..DbError::default()
    })
}

/// The 1-based character offset into `sql` that the `LINE n: …` excerpt and the caret line
/// below it point at. Long lines are cut, with `...` at the cut ends.
fn position(context: &str, sql: &str) -> Option<u32> {
    let mut lines = context.lines();
    let excerpt_line = lines.next()?;
    let caret_line = lines.next()?;
    let rest = excerpt_line.strip_prefix("LINE ")?;
    let (number, excerpt) = rest.split_once(": ")?;
    let prefix = excerpt_line.len() - excerpt.len();
    let line: usize = number.parse().ok()?;
    let caret = caret_line.find('^')?.checked_sub(prefix)?;

    let (excerpt, cut) = match excerpt.strip_prefix("...") {
        Some(rest) => (rest, 3),
        None => (excerpt, 0),
    };
    let excerpt = excerpt.strip_suffix("...").unwrap_or(excerpt);
    let caret = caret.checked_sub(cut)?;

    let mut before = 0;
    for (i, text) in sql.split('\n').enumerate() {
        if i + 1 == line {
            let start = text.find(excerpt.trim_end())?;
            let column = text[..start].chars().count() + caret;
            return u32::try_from(before + column + 1).ok();
        }
        before += text.chars().count() + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(text: &str, sql: &str) -> DbError {
        match from_text(text, sql) {
            Error::Database(e) => e,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reads_kind_hint_and_position() {
        let sql = "select * from nope";
        let e = db(
            "Catalog Error: Table with name nope does not exist!\nDid you mean \"pg_enum\"?\n\n\
             LINE 1: select * from nope\n                      ^^^^",
            sql,
        );
        assert_eq!(e.code.as_deref(), Some("Catalog"));
        assert_eq!(e.message, "Table with name nope does not exist!");
        assert_eq!(e.hint.as_deref(), Some("Did you mean \"pg_enum\"?"));
        assert_eq!(e.position, Some(15));
    }

    #[test]
    fn finds_positions_in_cut_and_later_lines() {
        let sql = "select 1,\n  é, xyz ";
        let e = db(
            "Binder Error: column xyz\n\nLINE 2:   é, xyz\n             ^",
            sql,
        );
        assert_eq!(e.position, Some(16));
        let sql = format!("select {}bad", "a, ".repeat(40));
        let e = db(
            "Parser Error: syntax error at or near \"bad\"\n\n\
             LINE 1: ...a, a, a, a, bad\n                       ^",
            &sql,
        );
        let at = usize::try_from(e.position.unwrap()).unwrap();
        assert_eq!(&sql[at - 1..], "bad");
    }

    #[test]
    fn interrupt_is_a_cancellation() {
        assert!(matches!(
            from_text("INTERRUPT Error: Interrupted!", "select 1"),
            Error::Cancelled
        ));
        let e = db("something odd", "select 1");
        assert_eq!((e.code, e.position), (None, None));
    }
}
