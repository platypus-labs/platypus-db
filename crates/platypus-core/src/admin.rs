//! What a database server is doing: its sessions, the statements they run and the locks
//! they hold or wait for. Read by [`Connection::server_sessions`] and
//! [`Connection::server_locks`]; [`Connection::end_server_session`] stops one. What it has
//! done: the statements that cost it most ([`Connection::top_queries`]) and the use of its
//! tables and indexes ([`Connection::object_statistics`]); and the upkeep of a table
//! ([`Connection::maintain`]).
//!
//! [`Connection::server_sessions`]: crate::Connection::server_sessions
//! [`Connection::server_locks`]: crate::Connection::server_locks
//! [`Connection::end_server_session`]: crate::Connection::end_server_session
//! [`Connection::top_queries`]: crate::Connection::top_queries
//! [`Connection::object_statistics`]: crate::Connection::object_statistics
//! [`Connection::maintain`]: crate::Connection::maintain

use serde::{Deserialize, Serialize};

use crate::{Connection, Error, ExecOptions, ExecResult, Result, TextRowExt, query_text};

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

/// How many statements [`Connection::top_queries`](crate::Connection::top_queries) returns.
pub const TOP_QUERIES: usize = 100;

/// A statement the server has run, summed over its runs since its statistics were last
/// reset. Statements that differ only in their values count as one.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ServerQuery {
    /// The statement, its values replaced as the server writes them (`$1`, `?`).
    pub query: String,
    /// One run of it with its values, where the server keeps one (MySQL 8): it can be
    /// explained as it is.
    pub sample: Option<String>,
    pub database: Option<String>,
    pub user: Option<String>,
    pub calls: u64,
    /// The time of all its runs, in seconds.
    pub total_seconds: f64,
    pub mean_seconds: f64,
    pub max_seconds: Option<f64>,
    /// The rows its runs returned or changed.
    pub rows: Option<u64>,
}

/// Whether [`ObjectStatistics`] are of a table or of an index.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StatisticsKind {
    #[default]
    Table,
    Index,
}

/// How a table or an index of the database has been used, from the server's statistics;
/// `None` where the server does not tell.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ObjectStatistics {
    pub schema: String,
    pub name: String,
    pub kind: StatisticsKind,
    /// The table of an index.
    pub table: Option<String>,
    /// On disk, with its indexes for a table.
    pub size_bytes: Option<u64>,
    /// The server's estimate.
    pub rows: Option<u64>,
    /// Rows deleted or updated whose space is not reclaimed yet (PostgreSQL).
    pub dead_rows: Option<u64>,
    /// Allocated and unused (MySQL's `DATA_FREE`).
    pub free_bytes: Option<u64>,
    /// Reads of a whole table: sequential scans.
    pub full_scans: Option<u64>,
    /// Reads through an index: of the table's indexes, or of this index.
    pub index_scans: Option<u64>,
    /// When it was last vacuumed (PostgreSQL), by hand or not.
    pub last_vacuum: Option<String>,
    /// When its statistics were last updated, by hand or not.
    pub last_analyze: Option<String>,
    /// A unique or primary key index, which constraints need even when no read uses it.
    pub unique: bool,
}

/// The upkeep of a table, by [`Connection::maintain`](crate::Connection::maintain). Each
/// database has its own: see [`Capabilities::maintenance`](crate::Capabilities::maintenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Maintenance {
    /// Reclaims the space of dead rows (PostgreSQL `VACUUM`).
    Vacuum,
    /// Updates the planner's statistics (`ANALYZE`, `UPDATE STATISTICS`).
    Analyze,
    /// Rebuilds the indexes (`REINDEX`, `ALTER INDEX ALL … REBUILD`).
    Reindex,
    /// Rebuilds the table and its indexes, reclaiming space (MySQL `OPTIMIZE TABLE`).
    Optimize,
    /// Checks the table for errors (MySQL `CHECK TABLE`, SQL Server `DBCC CHECKTABLE`).
    Check,
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

/// The statements `sql` reads, one a row: statement, a run of it with its values, database,
/// user, calls, then total, mean and longest time in seconds, and rows.
///
/// # Errors
/// When the query fails.
pub async fn query_statements(conn: &dyn Connection, sql: &str) -> Result<Vec<ServerQuery>> {
    Ok(query_text(conn, sql)
        .await?
        .into_iter()
        .map(|r| ServerQuery {
            query: r.text(0),
            sample: r.opt(1).filter(|s| !s.is_empty()),
            database: r.opt(2),
            user: r.opt(3),
            calls: r.number(4).unwrap_or(0),
            total_seconds: seconds_of(r.opt(5)).unwrap_or(0.0),
            mean_seconds: seconds_of(r.opt(6)).unwrap_or(0.0),
            max_seconds: seconds_of(r.opt(7)),
            rows: r.number(8),
        })
        .collect())
}

/// Refuses session `id` when it is the one asking, whose id `own` reads: ending it would
/// close the connection that asks, or cancel the very statement.
///
/// # Errors
/// When `id` is the session asking, or `own` fails.
pub async fn refuse_own_session(conn: &dyn Connection, own: &str, id: u64) -> Result<()> {
    let rows = query_text(conn, own).await?;
    if rows.first().and_then(|r| r.number(0)) == Some(id) {
        Err(Error::Other(format!(
            "session {id} is the one asking: it cannot end itself"
        )))
    } else {
        Ok(())
    }
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

/// Runs maintenance statement `sql` and returns what the server said: the rows it returned
/// (MySQL's `Table`, `Op`, `Msg_type`, `Msg_text`), then its messages (`VACUUM VERBOSE`,
/// `DBCC`), a line each.
///
/// # Errors
/// When the statement fails.
pub async fn run_maintenance(conn: &dyn Connection, sql: &str) -> Result<Vec<String>> {
    // Messages of earlier statements are not this one's.
    let _ = conn.take_messages();
    let mut lines = Vec::new();
    if let ExecResult::Rows(stream) = conn.execute(sql, &ExecOptions::default()).await? {
        for row in crate::metadata::text_rows(stream).await? {
            let cells: Vec<String> = row.into_iter().flatten().collect();
            lines.push(cells.join(" · "));
        }
    }
    lines.extend(conn.take_messages());
    Ok(lines)
}

/// Sorts `objects` by schema, then each table followed by its indexes.
pub fn sort_statistics(objects: &mut [ObjectStatistics]) {
    objects.sort_by(|a, b| {
        let key = |o: &ObjectStatistics| {
            (
                o.schema.clone(),
                o.table.clone().unwrap_or_else(|| o.name.clone()),
                o.kind == StatisticsKind::Index,
                o.name.clone(),
            )
        };
        key(a).cmp(&key(b))
    });
}

/// A time in seconds, as the server wrote it.
fn seconds_of(text: Option<String>) -> Option<f64> {
    text?.trim().parse().ok().filter(|s: &f64| s.is_finite())
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

    #[test]
    fn reads_seconds() {
        assert_eq!(seconds_of(Some(" 1.5 ".to_owned())), Some(1.5));
        assert_eq!(seconds_of(Some("NaN".to_owned())), None);
        assert_eq!(seconds_of(None), None);
    }
}
