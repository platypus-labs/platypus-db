//! Transaction bookkeeping shared by the drivers.
//!
//! In manual-commit mode a session opens a transaction before the first statement that
//! needs one and keeps it until COMMIT or ROLLBACK, whether the user types them or uses
//! [`Connection::commit`](crate::Connection::commit). Drivers learn about typed transaction
//! statements from [`classify`].

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TransactionState {
    /// Every statement commits on its own.
    pub autocommit: bool,
    /// Changes are pending (or the transaction failed) until COMMIT or ROLLBACK.
    pub open: bool,
}

/// What a statement does to the session's transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxEffect {
    Begin,
    /// Ends the transaction: COMMIT, ROLLBACK (not `ROLLBACK TO`), END, ABORT.
    End,
    /// DDL and friends, which MySQL commits implicitly.
    ImplicitCommit,
    None,
}

/// The first `n` words of `sql`, upper-cased, skipping whitespace and comments.
#[must_use]
pub fn leading_words(sql: &str, n: usize) -> Vec<String> {
    let mut words = Vec::with_capacity(n);
    let mut rest = sql;
    while words.len() < n {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("--").or_else(|| rest.strip_prefix('#')) {
            rest = after.find('\n').map_or("", |i| &after[i..]);
        } else if let Some(after) = rest.strip_prefix("/*") {
            rest = after.find("*/").map_or("", |i| &after[i + 2..]);
        } else {
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            if end == 0 {
                break;
            }
            words.push(rest[..end].to_ascii_uppercase());
            rest = &rest[end..];
        }
    }
    words
}

/// Classifies a single statement by its leading keywords.
#[must_use]
pub fn classify(sql: &str) -> TxEffect {
    let words = leading_words(sql, 2);
    let first = words.first().map_or("", String::as_str);
    let second = words.get(1).map_or("", String::as_str);
    match first {
        "BEGIN" if second != "ATOMIC" => TxEffect::Begin,
        "START" if second == "TRANSACTION" => TxEffect::Begin,
        "COMMIT" | "END" | "ABORT" => TxEffect::End,
        "ROLLBACK" if second != "TO" => TxEffect::End,
        "CREATE" | "ALTER" | "DROP" | "RENAME" | "TRUNCATE" | "GRANT" | "REVOKE" | "LOCK" => {
            TxEffect::ImplicitCommit
        }
        _ => TxEffect::None,
    }
}

/// Refusal to switch modes or close while changes are pending.
#[must_use]
pub fn open_transaction_error() -> crate::Error {
    crate::Error::Other("commit or roll back the open transaction first".to_owned())
}

/// Lock-free transaction flags for a driver's connection.
#[derive(Debug)]
pub struct TxTracker {
    autocommit: AtomicBool,
    open: AtomicBool,
}

impl Default for TxTracker {
    fn default() -> Self {
        Self {
            autocommit: AtomicBool::new(true),
            open: AtomicBool::new(false),
        }
    }
}

impl TxTracker {
    #[must_use]
    pub fn state(&self) -> TransactionState {
        TransactionState {
            autocommit: self.autocommit.load(Ordering::Acquire),
            open: self.open.load(Ordering::Acquire),
        }
    }

    /// Fails while a transaction is open: switching modes must not commit it silently.
    pub fn ensure_closed(&self) -> crate::Result<()> {
        if self.state().open {
            return Err(open_transaction_error());
        }
        Ok(())
    }

    pub fn set_autocommit(&self, on: bool) {
        self.autocommit.store(on, Ordering::Release);
    }

    pub fn set_open(&self, open: bool) {
        self.open.store(open, Ordering::Release);
    }

    /// Whether `effect` needs a `BEGIN` sent first: manual mode, nothing open yet, and a
    /// statement that is not itself transaction control. DDL counts: it is transactional in
    /// the drivers that ask (MySQL, which commits it, tracks its transactions itself).
    #[must_use]
    pub fn needs_begin(&self, effect: TxEffect) -> bool {
        let state = self.state();
        !state.autocommit
            && !state.open
            && matches!(effect, TxEffect::None | TxEffect::ImplicitCommit)
    }

    /// Updates the flags after a statement ran (successfully or not: a failed statement
    /// inside a transaction leaves it open, in PostgreSQL as an aborted transaction).
    pub fn after(&self, effect: TxEffect, implicit_commit: bool) {
        match effect {
            TxEffect::Begin => self.set_open(true),
            TxEffect::End => self.set_open(false),
            TxEffect::ImplicitCommit if implicit_commit => self.set_open(false),
            TxEffect::ImplicitCommit | TxEffect::None => {
                if !self.state().autocommit {
                    self.set_open(true);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_transaction_statements() {
        assert_eq!(classify("begin"), TxEffect::Begin);
        assert_eq!(classify("  -- note\n BEGIN TRANSACTION"), TxEffect::Begin);
        assert_eq!(classify("start transaction read only"), TxEffect::Begin);
        assert_eq!(classify("BEGIN ATOMIC SELECT 1; END"), TxEffect::None);
        assert_eq!(classify("commit"), TxEffect::End);
        assert_eq!(classify("/* x */ rollback"), TxEffect::End);
        assert_eq!(classify("ROLLBACK TO SAVEPOINT a"), TxEffect::None);
        assert_eq!(classify("end"), TxEffect::End);
        assert_eq!(
            classify("create table t (id int)"),
            TxEffect::ImplicitCommit
        );
        assert_eq!(classify("select 1"), TxEffect::None);
        assert_eq!(classify("savepoint a"), TxEffect::None);
        assert_eq!(classify(""), TxEffect::None);
    }

    #[test]
    fn tracks_manual_transactions() {
        let tx = TxTracker::default();
        assert!(!tx.needs_begin(TxEffect::None), "autocommit");
        tx.after(TxEffect::None, false);
        assert!(!tx.state().open);

        tx.set_autocommit(false);
        assert!(tx.needs_begin(TxEffect::None));
        assert!(!tx.needs_begin(TxEffect::Begin));
        assert!(
            tx.needs_begin(TxEffect::ImplicitCommit),
            "DDL opens the transaction"
        );
        tx.after(TxEffect::None, false);
        assert!(tx.state().open);
        assert!(!tx.needs_begin(TxEffect::None));
        tx.after(TxEffect::ImplicitCommit, true);
        assert!(!tx.state().open, "MySQL DDL commits");
        tx.after(TxEffect::ImplicitCommit, false);
        assert!(tx.state().open, "PostgreSQL DDL is transactional");
        tx.after(TxEffect::End, false);
        assert!(!tx.state().open);
    }
}
