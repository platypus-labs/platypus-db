//! What the driver reads in the statements it runs: transaction control, which Transact-SQL
//! spells its own way, and the position of syntax errors.
//!
//! The portable spellings the app and the user type (`BEGIN`, `SAVEPOINT x`, `RELEASE
//! SAVEPOINT x`, `ROLLBACK TO SAVEPOINT x`, used by the data editor) are translated: a bare
//! `BEGIN` is no statement in Transact-SQL, nor is `SAVEPOINT`, so the translation cannot
//! change the meaning of valid Transact-SQL.

use std::borrow::Cow;

use platypus_core::TxEffect;
use platypus_sql::{Dialect, Lexer};

/// A statement as it will be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared<'a> {
    /// `None` for a statement with nothing to do in Transact-SQL (`RELEASE SAVEPOINT`).
    pub sql: Option<Cow<'a, str>>,
    pub effect: TxEffect,
}

/// The words, quoted names and symbols of `sql`, without comments and a final `;`.
fn tokens(sql: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Lexer::new(sql, Dialect::SqlServer)
        .filter(|t| !t.is_trivia())
        .map(|t| t.text(sql))
        .collect();
    if out.last() == Some(&";") {
        out.pop();
    }
    out
}

fn is(token: Option<&&str>, words: &[&str]) -> bool {
    token.is_some_and(|t| words.iter().any(|w| w.eq_ignore_ascii_case(t)))
}

const TRANSACTION: &[&str] = &["TRAN", "TRANSACTION", "WORK"];

/// Classifies `sql` and translates the portable transaction statements.
#[must_use]
pub fn prepare(sql: &str) -> Prepared<'_> {
    let t = tokens(sql);
    let first = t.first();
    let second = t.get(1);
    let keep = |effect| Prepared {
        sql: Some(Cow::Borrowed(sql)),
        effect,
    };
    let send = |text: String, effect| Prepared {
        sql: Some(Cow::Owned(text)),
        effect,
    };
    if is(first, &["BEGIN"]) {
        return match t.len() {
            // `BEGIN`, `BEGIN WORK`: the standard spelling.
            1 => send("BEGIN TRANSACTION".to_owned(), TxEffect::Begin),
            2 if is(second, &["WORK"]) => send("BEGIN TRANSACTION".to_owned(), TxEffect::Begin),
            _ if is(second, &["TRAN", "TRANSACTION", "DISTRIBUTED"]) => keep(TxEffect::Begin),
            // A block: `BEGIN ... END`, `BEGIN TRY`.
            _ => keep(TxEffect::None),
        };
    }
    if is(first, &["START"]) && is(second, &["TRANSACTION"]) {
        return send("BEGIN TRANSACTION".to_owned(), TxEffect::Begin);
    }
    if is(first, &["COMMIT"]) {
        return keep(TxEffect::End);
    }
    if is(first, &["ROLLBACK"]) {
        if is(second, &["TO"]) {
            // `ROLLBACK TO [SAVEPOINT] name`.
            let name = t.get(2..).unwrap_or_default();
            let name = match name.first() {
                Some(w) if w.eq_ignore_ascii_case("SAVEPOINT") => &name[1..],
                _ => name,
            };
            return send(
                format!("ROLLBACK TRANSACTION {}", name.join(" ")),
                TxEffect::None,
            );
        }
        let rest = if is(second, TRANSACTION) {
            &t[2..]
        } else {
            &t[1..]
        };
        // `ROLLBACK TRAN name` may name a savepoint, which leaves the transaction open.
        return keep(if rest.is_empty() {
            TxEffect::End
        } else {
            TxEffect::None
        });
    }
    if is(first, &["SAVEPOINT"]) && t.len() > 1 {
        return send(
            format!("SAVE TRANSACTION {}", t[1..].join(" ")),
            TxEffect::None,
        );
    }
    if is(first, &["RELEASE"]) && (t.len() == 2 || is(second, &["SAVEPOINT"])) {
        // Transact-SQL savepoints are released with their transaction.
        return Prepared {
            sql: None,
            effect: TxEffect::None,
        };
    }
    keep(TxEffect::None)
}

/// The 1-based character position of an error on `line` (1-based) of `sql`: the token the
/// message quotes (`Incorrect syntax near 'x'.`) when that line has it, else the line's
/// first character.
#[must_use]
pub fn error_position(sql: &str, line: u32, message: &str) -> Option<u32> {
    let index = usize::try_from(line.checked_sub(1)?).ok()?;
    let mut start = 0;
    for _ in 0..index {
        start += sql[start..].find('\n')? + 1;
    }
    let end = sql[start..].find('\n').map_or(sql.len(), |i| start + i);
    let text = &sql[start..end];
    let near = message
        .split_once("near '")
        .and_then(|(_, rest)| rest.rsplit_once('\''))
        .map(|(snippet, _)| snippet)
        .filter(|s| !s.is_empty());
    let offset = near
        .and_then(|snippet| text.find(snippet))
        .unwrap_or_else(|| text.len() - text.trim_start().len());
    u32::try_from(sql[..start + offset].chars().count() + 1).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent(sql: &str) -> Option<String> {
        prepare(sql).sql.map(Cow::into_owned)
    }

    #[test]
    fn translates_portable_transaction_statements() {
        assert_eq!(sent("BEGIN").as_deref(), Some("BEGIN TRANSACTION"));
        assert_eq!(sent("begin work;").as_deref(), Some("BEGIN TRANSACTION"));
        assert_eq!(
            sent("START TRANSACTION").as_deref(),
            Some("BEGIN TRANSACTION")
        );
        assert_eq!(
            sent("SAVEPOINT platypus_edit").as_deref(),
            Some("SAVE TRANSACTION platypus_edit")
        );
        assert_eq!(
            sent("ROLLBACK TO SAVEPOINT platypus_edit").as_deref(),
            Some("ROLLBACK TRANSACTION platypus_edit")
        );
        assert_eq!(sent("RELEASE SAVEPOINT platypus_edit"), None);
        assert_eq!(sent("BEGIN TRAN t1").as_deref(), Some("BEGIN TRAN t1"));
    }

    #[test]
    fn classifies_transaction_statements() {
        let effect = |sql| prepare(sql).effect;
        assert_eq!(effect("BEGIN"), TxEffect::Begin);
        assert_eq!(effect("begin transaction"), TxEffect::Begin);
        assert_eq!(effect("BEGIN DISTRIBUTED TRAN"), TxEffect::Begin);
        assert_eq!(
            effect("BEGIN TRY SELECT 1 END TRY BEGIN CATCH END CATCH"),
            TxEffect::None
        );
        assert_eq!(effect("BEGIN SELECT 1 END"), TxEffect::None);
        assert_eq!(effect("COMMIT"), TxEffect::End);
        assert_eq!(effect("commit tran"), TxEffect::End);
        assert_eq!(effect("ROLLBACK"), TxEffect::End);
        assert_eq!(effect("ROLLBACK TRANSACTION;"), TxEffect::End);
        assert_eq!(effect("ROLLBACK TRAN sp1"), TxEffect::None);
        assert_eq!(effect("-- note\nCREATE TABLE t (id INT)"), TxEffect::None);
        assert_eq!(effect("END"), TxEffect::None);
        assert_eq!(effect("SELECT 1"), TxEffect::None);
    }

    #[test]
    fn locates_errors() {
        let sql = "SELECT * FROMM t";
        let message = "Incorrect syntax near 'FROMM'.";
        assert_eq!(error_position(sql, 1, message), Some(10));
        let sql = "SELECT 1;\n  SELECT * FROM nope";
        assert_eq!(
            error_position(sql, 2, "Invalid object name 'nope'."),
            Some(13),
            "the line's first character"
        );
        assert_eq!(error_position("é\nx y", 2, "near 'y'"), Some(5));
        assert_eq!(error_position(sql, 0, message), None);
        assert_eq!(error_position(sql, 7, message), None);
    }
}
