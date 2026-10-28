//! What a database server is doing: its sessions, the statements they run and the locks
//! they hold or wait for. Read by [`Connection::server_sessions`] and
//! [`Connection::server_locks`]; [`Connection::end_server_session`] stops one.
//!
//! [`Connection::server_sessions`]: crate::Connection::server_sessions
//! [`Connection::server_locks`]: crate::Connection::server_locks
//! [`Connection::end_server_session`]: crate::Connection::end_server_session

use serde::{Deserialize, Serialize};

use crate::{Connection, Error, Result, TextRowExt, query_text};

/// A session of the server, with the statement it runs, if any.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ServerSession {
    /// The server's id for it: PostgreSQL's process id, MySQL's connection id, SQL
    /// Server's session id.
    pub id: String,
    pub user: Option<String>,
    pub database: Option<String>,
    /// The client's address (and port, where the server tells it).
    pub client: Option<String>,
    pub application: Option<String>,
    /// As the server names it: `active`, `idle in transaction`, `Sleep`, `running`, …
    pub state: Option<String>,
    /// When the session logged in, as the server writes the time.
    pub started: Option<String>,
    /// The statement running, or the last one where the server keeps it (PostgreSQL).
    pub query: Option<String>,
    /// How long the statement has been running, in seconds; `None` when none runs.
    pub query_seconds: Option<f64>,
    /// What it waits for, e.g. `Lock:transactionid` or `LCK_M_X`.
    pub wait: Option<String>,
    /// The sessions it waits for, by id.
    pub blocked_by: Vec<String>,
    /// The session that asked.
    pub current: bool,
}

impl ServerSession {
    /// Whether it runs a statement: idle sessions have none, or only their last one.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.query_seconds.is_some()
    }
}

/// A lock a session holds, or waits for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ServerLock {
    /// The id of the session, as in [`ServerSession::id`].
    pub session: String,
    /// What is locked, as the server calls it: `relation`, `transactionid`, `RECORD`, `KEY`, …
    pub kind: String,
    /// The table it is on, `schema.table`, where the server tells.
    pub object: Option<String>,
    pub mode: String,
    /// Held; otherwise waited for.
    pub granted: bool,
}

/// How [`Connection::end_server_session`](crate::Connection::end_server_session) stops
/// a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EndSession {
    /// Cancels the statement it runs; the session stays.
    Cancel,
    /// Closes the session, rolling back its transaction.
    Terminate,
}

/// The locks `sql` reads, one a row: session id, kind, object, mode, and whether held.
///
/// # Errors
/// When the query fails.
pub async fn query_locks(conn: &dyn Connection, sql: &str) -> Result<Vec<ServerLock>> {
    Ok(query_text(conn, sql)
        .await?
        .into_iter()
        .map(|r| ServerLock {
            session: r.text(0),
            kind: r.text(1),
            object: r.opt(2),
            mode: r.text(3),
            granted: r.flag(4),
        })
        .collect())
}

/// `id` as the number every server's ids are, refused otherwise: it goes into the SQL.
///
/// # Errors
/// When `id` is not a whole number.
pub fn session_number(id: &str) -> Result<u64> {
    id.trim()
        .parse()
        .map_err(|_| Error::Other(format!("`{id}` is not a session id")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_numbers() {
        assert_eq!(session_number(" 42 ").unwrap(), 42);
        assert!(session_number("1; DROP TABLE t").is_err());
        assert!(session_number("-1").is_err());
    }
}
