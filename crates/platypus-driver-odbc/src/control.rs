//! Transaction statements the bridge runs through the ODBC API instead of sending them:
//! drivers take `BEGIN` differently or not at all, and Oracle reads a bare `BEGIN` as the
//! start of a PL/SQL block.

use platypus_core::{TxEffect, classify, leading_words};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Begin,
    Commit,
    Rollback,
}

/// The transaction statement `sql` is, if it is only that: `BEGIN [WORK | TRANSACTION]`,
/// `START TRANSACTION`, `COMMIT [WORK]`, `ROLLBACK [WORK]`. A PL/SQL block starting with
/// `BEGIN` has more words.
#[must_use]
pub fn control(sql: &str) -> Option<Control> {
    let words = leading_words(sql, 3);
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let noise = |w: &str| matches!(w, "WORK" | "TRANSACTION" | "TRAN");
    match words.as_slice() {
        ["BEGIN"] | ["START", "TRANSACTION"] => Some(Control::Begin),
        ["BEGIN", w] if noise(w) => Some(Control::Begin),
        ["COMMIT"] => Some(Control::Commit),
        ["COMMIT", w] if noise(w) => Some(Control::Commit),
        ["ROLLBACK"] => Some(Control::Rollback),
        ["ROLLBACK", w] if noise(w) => Some(Control::Rollback),
        _ => None,
    }
}

/// What `sql` does to the session's transaction.
#[must_use]
pub fn effect(sql: &str) -> TxEffect {
    match control(sql) {
        Some(Control::Begin) => TxEffect::Begin,
        Some(Control::Commit | Control::Rollback) => TxEffect::End,
        None => match classify(sql) {
            // A PL/SQL block, or Db2 compound SQL.
            TxEffect::Begin => TxEffect::None,
            other => other,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_statements() {
        assert_eq!(control("begin"), Some(Control::Begin));
        assert_eq!(control("BEGIN TRANSACTION"), Some(Control::Begin));
        assert_eq!(control("start transaction"), Some(Control::Begin));
        assert_eq!(control("commit;"), Some(Control::Commit));
        assert_eq!(control("-- done\nCOMMIT WORK"), Some(Control::Commit));
        assert_eq!(control("rollback"), Some(Control::Rollback));
        assert_eq!(control("ROLLBACK TO SAVEPOINT a"), None);
        assert_eq!(control("BEGIN NULL; END;"), None);
        assert_eq!(control("begin dbms_output.put_line('x'); end;"), None);
        assert_eq!(control("select 1 from dual"), None);
        assert_eq!(effect("BEGIN NULL; END;"), TxEffect::None);
        assert_eq!(effect("begin"), TxEffect::Begin);
        assert_eq!(effect("commit"), TxEffect::End);
        assert_eq!(effect("create table t (a int)"), TxEffect::ImplicitCommit);
    }
}
