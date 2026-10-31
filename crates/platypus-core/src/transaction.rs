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
    /// Cannot run inside a transaction: `CREATE DATABASE`, `VACUUM`, `... CONCURRENTLY`.
    Outside,
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
    let words = leading_words(sql, 4);
    let first = words.first().map_or("", String::as_str);
    let second = words.get(1).map_or("", String::as_str);
    let outside = matches!(
        (first, second),
        ("CREATE" | "DROP", "DATABASE" | "TABLESPACE") | ("ALTER", "SYSTEM") | ("VACUUM", _)
    ) || (matches!(first, "CREATE" | "DROP" | "REINDEX")
        && words.iter().any(|w| w == "CONCURRENTLY"));
    if outside {
        return TxEffect::Outside;
    }
    match first {
        "BEGIN" if second != "ATOMIC" => TxEffect::Begin,
        "START" if second == "TRANSACTION" => TxEffect::Begin,
        "COMMIT" | "END" | "ABORT" => TxEffect::End,
        "ROLLBACK" if second != "TO" => TxEffect::End,
        // MySQL commits no temporary table.
        "CREATE" | "DROP" if matches!(second, "TEMPORARY" | "TEMP") => TxEffect::None,
        "CREATE" | "ALTER" | "DROP" | "RENAME" | "TRUNCATE" | "GRANT" | "REVOKE" | "LOCK" => {
            TxEffect::ImplicitCommit
        }
        _ => TxEffect::None,
    }
}

/// The commit mode a typed statement sets for the session: MySQL's `SET autocommit = 0`
/// (`SESSION`, `@@autocommit`, `ON`/`OFF` too) and Transact-SQL's
/// `SET IMPLICIT_TRANSACTIONS ON`, which turns autocommit off. `None` for any other statement,
/// `SET GLOBAL autocommit` included: it changes the next sessions, not this one.
#[must_use]
pub fn typed_autocommit(sql: &str) -> Option<bool> {
    // `@@session.autocommit := 0` as words: SESSION AUTOCOMMIT 0.
    let words = leading_words(&sql.replace(['@', '.', '=', ':'], " "), 4);
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let value = |v: &str| match v {
        "1" | "ON" | "TRUE" => Some(true),
        "0" | "OFF" | "FALSE" => Some(false),
        _ => None,
    };
    match words.as_slice() {
        ["SET", "AUTOCOMMIT", v, ..] | ["SET", "SESSION" | "LOCAL", "AUTOCOMMIT", v] => value(v),
        ["SET", "IMPLICIT_TRANSACTIONS", v, ..] => value(v).map(|on| !on),
        _ => None,
    }
}

/// A transaction statement that drivers run through their client API instead of sending it:
/// ODBC drivers take `BEGIN` differently or not at all, and Oracle reads a bare `BEGIN` as
/// the start of a PL/SQL block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxControl {
    Begin,
    Commit,
    Rollback,
}

impl TxControl {
    /// The transaction statement `sql` is, if it is only that: `BEGIN [WORK | TRANSACTION]`,
    /// `START TRANSACTION`, `COMMIT [WORK]`, `ROLLBACK [WORK]`. A PL/SQL block starting
    /// with `BEGIN` has more words.
    #[must_use]
    pub fn of(sql: &str) -> Option<Self> {
        let words = leading_words(sql, 3);
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let noise = |w: &str| matches!(w, "WORK" | "TRANSACTION" | "TRAN");
        match words.as_slice() {
            ["BEGIN"] | ["START", "TRANSACTION"] => Some(Self::Begin),
            ["BEGIN", w] if noise(w) => Some(Self::Begin),
            ["COMMIT"] => Some(Self::Commit),
            ["COMMIT", w] if noise(w) => Some(Self::Commit),
            ["ROLLBACK"] => Some(Self::Rollback),
            ["ROLLBACK", w] if noise(w) => Some(Self::Rollback),
            _ => None,
        }
    }

    /// What `sql` does to the transaction of a driver that runs [`TxControl`] statements
    /// itself: a longer statement starting with `BEGIN` is a block (PL/SQL, Db2 compound
    /// SQL), not a transaction.
    #[must_use]
    pub fn effect(sql: &str) -> TxEffect {
        match Self::of(sql) {
            Some(Self::Begin) => TxEffect::Begin,
            Some(Self::Commit | Self::Rollback) => TxEffect::End,
            None => match classify(sql) {
                TxEffect::Begin => TxEffect::None,
                other => other,
            },
        }
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

    /// After a typed statement that set the commit mode (see [`typed_autocommit`]): back in
    /// autocommit mode, an open transaction ends where the server `commits` it on the switch
    /// (MySQL) and stays open where it does not (SQL Server keeps `@@TRANCOUNT`).
    pub fn typed_autocommit(&self, on: bool, commits: bool) {
        self.set_autocommit(on);
        if on && commits {
            self.set_open(false);
        }
    }

    /// Whether `effect` needs a `BEGIN` sent first: manual mode, nothing open yet, and a
    /// statement that is not itself transaction control nor refuses to run in one. DDL counts:
    /// it is transactional in the drivers that ask (MySQL commits it, and does not ask).
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
            TxEffect::ImplicitCommit | TxEffect::Outside if implicit_commit => self.set_open(false),
            // Ran on its own: it neither opened nor ended a transaction.
            TxEffect::Outside => {}
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
        assert_eq!(
            classify("CREATE TEMPORARY TABLE t AS SELECT 1"),
            TxEffect::None
        );
        assert_eq!(classify("drop temporary table t"), TxEffect::None);
        assert_eq!(classify("CREATE DATABASE x"), TxEffect::Outside);
        assert_eq!(classify("vacuum analyze t"), TxEffect::Outside);
        assert_eq!(
            classify("CREATE UNIQUE INDEX CONCURRENTLY i ON t (a)"),
            TxEffect::Outside
        );
        assert_eq!(
            classify("ALTER SYSTEM SET work_mem = '8MB'"),
            TxEffect::Outside
        );
        assert_eq!(classify("select 1"), TxEffect::None);
        assert_eq!(classify("savepoint a"), TxEffect::None);
        assert_eq!(classify(""), TxEffect::None);
    }

    #[test]
    fn transaction_statements_run_through_the_api() {
        assert_eq!(TxControl::of("begin"), Some(TxControl::Begin));
        assert_eq!(TxControl::of("BEGIN TRANSACTION"), Some(TxControl::Begin));
        assert_eq!(TxControl::of("start transaction"), Some(TxControl::Begin));
        assert_eq!(TxControl::of("commit;"), Some(TxControl::Commit));
        assert_eq!(
            TxControl::of("-- done\nCOMMIT WORK"),
            Some(TxControl::Commit)
        );
        assert_eq!(TxControl::of("rollback"), Some(TxControl::Rollback));
        assert_eq!(TxControl::of("ROLLBACK TO SAVEPOINT a"), None);
        assert_eq!(TxControl::of("BEGIN NULL; END;"), None);
        assert_eq!(TxControl::of("begin dbms_output.put_line('x'); end;"), None);
        assert_eq!(TxControl::of("select 1 from dual"), None);
        assert_eq!(TxControl::effect("BEGIN NULL; END;"), TxEffect::None);
        assert_eq!(TxControl::effect("begin"), TxEffect::Begin);
        assert_eq!(TxControl::effect("commit"), TxEffect::End);
        assert_eq!(
            TxControl::effect("create table t (a int)"),
            TxEffect::ImplicitCommit
        );
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
        assert!(!tx.needs_begin(TxEffect::Outside));
        tx.after(TxEffect::Outside, false);
        assert!(!tx.state().open, "CREATE DATABASE runs on its own");
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

    #[test]
    fn reads_typed_commit_modes() {
        assert_eq!(typed_autocommit("SET autocommit = 0"), Some(false));
        assert_eq!(typed_autocommit("set autocommit=1;"), Some(true));
        assert_eq!(
            typed_autocommit("SET SESSION autocommit = OFF"),
            Some(false)
        );
        assert_eq!(typed_autocommit("-- x\nSET @@autocommit := ON"), Some(true));
        assert_eq!(
            typed_autocommit("SET @@session.autocommit = 0"),
            Some(false)
        );
        assert_eq!(
            typed_autocommit("SET IMPLICIT_TRANSACTIONS ON"),
            Some(false)
        );
        assert_eq!(
            typed_autocommit("set implicit_transactions off"),
            Some(true)
        );
        assert_eq!(typed_autocommit("SET GLOBAL autocommit = 0"), None);
        assert_eq!(typed_autocommit("SET @@global.autocommit = 0"), None);
        assert_eq!(typed_autocommit("SET NOCOUNT ON"), None);
        assert_eq!(typed_autocommit("SET autocommit = @x"), None);
        assert_eq!(typed_autocommit("SELECT 'SET autocommit = 0'"), None);
    }

    #[test]
    fn typed_commit_modes_follow_the_server() {
        let tx = TxTracker::default();
        tx.typed_autocommit(false, true);
        assert!(!tx.state().autocommit);
        tx.after(TxEffect::None, true);
        assert!(
            tx.state().open,
            "the INSERT after SET autocommit = 0 waits for COMMIT"
        );
        tx.typed_autocommit(true, false);
        assert!(tx.state().open, "SQL Server keeps the transaction open");
        tx.typed_autocommit(true, true);
        assert!(!tx.state().open, "MySQL commits it");
    }
}
