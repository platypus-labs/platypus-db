//! Stopping a statement that Cancel did not stop. A break (`OCIBreak`) interrupts the server
//! with out-of-band data, which some networks lose (Docker's port forwarding among them): a
//! PL/SQL call waiting on the server (`DBMS_SESSION.SLEEP`, a lock) then runs to its end.
//! After the break, Cancel waits a little; a statement still running is cancelled from a
//! second session (`ALTER SYSTEM CANCEL SQL`, which needs the privilege), and failing that
//! the session is given up: its statement answers at once with [`abandoned`], the connection
//! counts as closed (the app opens a new one) and the old session ends, rolling back its
//! transaction, as soon as the server lets go of it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc as std_mpsc};
use std::time::Duration;

use oracle::{Connection, Connector};
use platypus_core::{Error, Result};
use tokio::sync::Notify;

use crate::oracle_error;
use crate::session::{Command, Login};

/// How long a break has to stop the statement.
const BREAK_WAIT: Duration = Duration::from_secs(2);
/// How long a cancel from another session has.
const CANCEL_WAIT: Duration = Duration::from_secs(3);

/// The error of a statement whose session was given up.
pub const ABANDONED: &str = "The statement did not stop on Cancel, so its connection was \
     closed and a new one is opened; the open transaction was rolled back.";

#[must_use]
pub fn abandoned() -> Error {
    Error::Other(ABANDONED.to_owned())
}

/// The statement a session runs: its number while it waits on the server, 0 otherwise.
#[derive(Debug, Default, Clone)]
pub struct Busy(Arc<AtomicU64>);

impl Busy {
    /// Runs `call`, a call to the server for statement `id`.
    pub fn call<T>(&self, id: u64, call: impl FnOnce() -> T) -> T {
        self.0.store(id, Ordering::SeqCst);
        let result = call();
        self.0.store(0, Ordering::SeqCst);
        result
    }

    #[must_use]
    pub fn running(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    /// Whether statement `id` is over (or waits on the app, not the server) within `limit`.
    pub async fn ends(&self, id: u64, limit: Duration) -> bool {
        let start = std::time::Instant::now();
        loop {
            if self.running() != id {
                return true;
            }
            if start.elapsed() >= limit {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// The statements that stop session `sid`, `serial` from another one: its statement (19c),
/// else the session itself.
#[must_use]
pub fn stop_statements(sid: u64, serial: u64) -> [String; 2] {
    [
        format!("ALTER SYSTEM CANCEL SQL '{sid}, {serial}'"),
        format!("ALTER SYSTEM KILL SESSION '{sid},{serial}' IMMEDIATE"),
    ]
}

/// What stopped the session's statement from another session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stopped {
    Statement,
    Session,
}

/// Stops the statement of session `sid`, `serial` from a session of its own, opened with
/// `login`.
fn stop(login: &Login, sid: u64, serial: u64) -> Result<Stopped> {
    let mut connector = Connector::new(&login.user, &login.password, &login.connect_string);
    connector.external_auth(login.user.is_empty());
    if let Some(privilege) = login.privilege {
        connector.privilege(privilege);
    }
    let connection = connector.connect().map_err(|e| oracle_error(&e))?;
    let [statement, session] = stop_statements(sid, serial);
    let stopped = match connection.execute(&statement, &[]) {
        Ok(_) => Ok(Stopped::Statement),
        Err(_) => connection
            .execute(&session, &[])
            .map(|_| Stopped::Session)
            .map_err(|e| oracle_error(&e)),
    };
    let _ = connection.close();
    stopped
}

/// What stops a session's statement, shared by the connection and its cancel handles.
pub struct Stopper {
    /// The session's connection: a break goes through it from any thread.
    pub connection: Arc<Connection>,
    pub busy: Busy,
    /// The session's `SID` and `SERIAL#`, when it could read them.
    pub ids: Option<(u64, u64)>,
    pub login: Login,
    /// The commands to the session's thread; none once it is given up.
    pub commands: Mutex<Option<std_mpsc::Sender<Command>>>,
    abandoned: AtomicBool,
    /// Wakes the statement waiting on a session given up.
    pub given_up: Notify,
}

impl Stopper {
    #[must_use]
    pub fn new(
        connection: Arc<Connection>,
        busy: Busy,
        ids: Option<(u64, u64)>,
        login: Login,
        commands: std_mpsc::Sender<Command>,
    ) -> Self {
        Self {
            connection,
            busy,
            ids,
            login,
            commands: Mutex::new(Some(commands)),
            abandoned: AtomicBool::new(false),
            given_up: Notify::new(),
        }
    }

    #[must_use]
    pub fn abandoned(&self) -> bool {
        self.abandoned.load(Ordering::SeqCst)
    }

    /// Gives the session up: no more commands go to it, the waiting statement answers.
    pub fn abandon(&self) {
        self.abandoned.store(true, Ordering::SeqCst);
        self.commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.given_up.notify_waiters();
    }

    /// Breaks the running statement, then stops it harder if it goes on (see the module).
    pub async fn cancel(self: Arc<Self>) -> Result<()> {
        let running = self.busy.running();
        let connection = Arc::clone(&self.connection);
        tokio::task::spawn_blocking(move || connection.break_execution())
            .await
            .map_err(|e| Error::Other(e.to_string()))?
            .map_err(|e| oracle_error(&e))?;
        if running == 0 || self.busy.ends(running, BREAK_WAIT).await {
            return Ok(());
        }
        if let Some((sid, serial)) = self.ids {
            let this = Arc::clone(&self);
            let outcome = tokio::task::spawn_blocking(move || stop(&this.login, sid, serial)).await;
            if matches!(outcome, Ok(Ok(Stopped::Statement)))
                && self.busy.ends(running, CANCEL_WAIT).await
            {
                return Ok(());
            }
        }
        self.abandon();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_that_stop_a_session() {
        assert_eq!(
            stop_statements(12, 3456),
            [
                "ALTER SYSTEM CANCEL SQL '12, 3456'",
                "ALTER SYSTEM KILL SESSION '12,3456' IMMEDIATE"
            ]
        );
    }

    #[tokio::test]
    async fn waits_for_the_statement_to_end() {
        let busy = Busy::default();
        assert!(busy.ends(1, Duration::ZERO).await, "nothing runs");
        busy.0.store(7, Ordering::SeqCst);
        assert!(!busy.ends(7, Duration::from_millis(100)).await);
        assert!(busy.ends(6, Duration::ZERO).await, "another statement");
        let other = busy.clone();
        let ended = tokio::spawn(async move { other.ends(7, Duration::from_secs(5)).await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(busy.call(8, || 42), 42);
        assert!(ended.await.unwrap());
        assert_eq!(busy.running(), 0);
    }
}
