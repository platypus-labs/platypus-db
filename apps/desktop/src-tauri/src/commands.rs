//! Tauri commands: the whole API the web UI uses.

// Tauri injects `State` and deserialized arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use platypus_core::{
    Connection, ConnectionConfig, Driver, DriverKind, Error, ObjectMeta, SchemaMeta, Secret,
    ServerInfo, TableDetails,
};
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_pg::PgDriver;
use platypus_driver_sqlite::SqliteDriver;
use platypus_sql::Dialect;
use platypus_store::{
    ConnectionStore, HistoryEntry, HistoryQuery, HistoryStatus, HistoryStore, NewHistoryEntry,
    PasswordUpdate, SavedConnection,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::{Channel, Response};
use uuid::Uuid;

use crate::display;
use crate::error::{CommandError, CommandResult};
use crate::jobs::{Job, Outcome, QueryEvent};

#[derive(Debug)]
pub struct AppState {
    /// `Err` when the connections file exists but cannot be read; shown instead of wiping it.
    store: Mutex<Result<ConnectionStore, String>>,
    /// Shared with job tasks, which record outcomes as they happen.
    history: Arc<Mutex<Result<HistoryStore, String>>>,
    sessions: Mutex<HashMap<u64, Session>>,
    jobs: Mutex<HashMap<u64, Arc<Job>>>,
    next_id: AtomicU64,
}

#[derive(Debug, Clone)]
struct Session {
    connection_id: Uuid,
    conn: Arc<dyn Connection>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl AppState {
    pub fn new(
        store: Result<ConnectionStore, String>,
        history: Result<HistoryStore, String>,
    ) -> Self {
        Self {
            store: Mutex::new(store),
            history: Arc::new(Mutex::new(history)),
            sessions: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn with_history<T>(
        &self,
        f: impl FnOnce(&HistoryStore) -> Result<T, platypus_store::StoreError>,
    ) -> CommandResult<T> {
        match &*lock(&self.history) {
            Ok(history) => f(history).map_err(Into::into),
            Err(e) => Err(CommandError::new("store", e.clone())),
        }
    }

    fn with_store<T>(
        &self,
        f: impl FnOnce(&mut ConnectionStore) -> Result<T, platypus_store::StoreError>,
    ) -> CommandResult<T> {
        match &mut *lock(&self.store) {
            Ok(store) => f(store).map_err(Into::into),
            Err(e) => Err(CommandError::new("store", e.clone())),
        }
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn session(&self, id: u64) -> CommandResult<Session> {
        lock(&self.sessions)
            .get(&id)
            .cloned()
            .ok_or_else(|| CommandError::not_found("session", id))
    }

    fn job(&self, id: u64) -> CommandResult<Arc<Job>> {
        lock(&self.jobs)
            .get(&id)
            .cloned()
            .ok_or_else(|| CommandError::not_found("query", id))
    }
}

async fn connect(config: &ConnectionConfig) -> Result<Box<dyn Connection>, Error> {
    match config.driver {
        DriverKind::Postgres => PgDriver.connect(config).await,
        DriverKind::MySql => MySqlDriver.connect(config).await,
        DriverKind::Sqlite => SqliteDriver.connect(config).await,
    }
}

#[derive(Debug, Serialize)]
pub struct AppInfo {
    name: &'static str,
    version: &'static str,
    os: &'static str,
    arch: &'static str,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "PlatypusDB",
        version: platypus_core::VERSION,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

/// What to do with the stored password when saving or testing a connection.
#[derive(Debug, Deserialize)]
#[serde(tag = "action", content = "value", rename_all = "snake_case")]
pub enum PasswordInput {
    Keep,
    Set(String),
    Clear,
}

#[tauri::command]
pub fn list_connections(state: State<'_, AppState>) -> CommandResult<Vec<SavedConnection>> {
    state.with_store(|s| Ok(s.list().to_vec()))
}

#[tauri::command]
pub fn default_connection(driver: DriverKind) -> SavedConnection {
    SavedConnection::new("", driver)
}

#[tauri::command]
pub fn save_connection(
    state: State<'_, AppState>,
    connection: SavedConnection,
    password: PasswordInput,
) -> CommandResult<Vec<SavedConnection>> {
    let update = match password {
        PasswordInput::Keep => PasswordUpdate::Keep,
        PasswordInput::Set(p) => PasswordUpdate::Set(Secret::new(p)),
        PasswordInput::Clear => PasswordUpdate::Clear,
    };
    state.with_store(|s| {
        s.save(connection, update)?;
        Ok(s.list().to_vec())
    })
}

#[tauri::command]
pub fn delete_connection(
    state: State<'_, AppState>,
    id: Uuid,
) -> CommandResult<Vec<SavedConnection>> {
    state.with_store(|s| {
        s.delete(id)?;
        Ok(s.list().to_vec())
    })
}

/// Connects with the form's current values, without saving them.
#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    connection: SavedConnection,
    password: PasswordInput,
) -> CommandResult<ServerInfo> {
    let secret = match password {
        PasswordInput::Set(p) => Some(Secret::new(p)),
        PasswordInput::Clear => None,
        // A connection that was never saved has nothing stored yet.
        PasswordInput::Keep => state.with_store(|s| match s.get(connection.id) {
            Some(_) => s.password(connection.id),
            None => Ok(None),
        })?,
    };
    let conn = connect(&connection.config(secret)).await?;
    Ok(conn.server_info().clone())
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    session_id: u64,
    server: ServerInfo,
}

/// Opens a session for a saved connection. `password` overrides the stored one.
#[tauri::command]
pub async fn open_session(
    state: State<'_, AppState>,
    connection_id: Uuid,
    password: Option<String>,
) -> CommandResult<SessionInfo> {
    let config = state.with_store(|s| {
        let saved = s
            .get(connection_id)
            .ok_or(platypus_store::StoreError::NotFound(connection_id))?
            .clone();
        let secret = match password {
            Some(p) => Some(Secret::new(p)),
            None => s.password(connection_id)?,
        };
        Ok(saved.config(secret))
    })?;
    let conn: Arc<dyn Connection> = Arc::from(connect(&config).await?);
    let server = conn.server_info().clone();
    let session_id = state.next_id();
    lock(&state.sessions).insert(
        session_id,
        Session {
            connection_id,
            conn,
        },
    );
    Ok(SessionInfo { session_id, server })
}

#[tauri::command]
pub fn close_session(state: State<'_, AppState>, session_id: u64) {
    lock(&state.sessions).remove(&session_id);
}

/// Starts a statement and returns its job id at once; progress arrives on `on_event`.
/// `limit` caps the rows fetched and frees the session afterwards (script mode).
#[tauri::command]
pub fn run_query(
    state: State<'_, AppState>,
    session_id: u64,
    sql: String,
    limit: Option<usize>,
    on_event: Channel<QueryEvent>,
) -> CommandResult<u64> {
    let session = state.session(session_id)?;
    let history = Arc::clone(&state.history);
    let started_at = SystemTime::now();
    let entry_sql = sql.clone();
    let record = move |outcome: Outcome| {
        let (duration, rows, status, error) = match outcome {
            Outcome::Rows { elapsed } => (elapsed, None, HistoryStatus::Ok, None),
            Outcome::Affected { rows, elapsed } => (elapsed, Some(rows), HistoryStatus::Ok, None),
            Outcome::Failed {
                error: Error::Cancelled,
                elapsed,
            } => (elapsed, None, HistoryStatus::Cancelled, None),
            Outcome::Failed { error, elapsed } => {
                (elapsed, None, HistoryStatus::Error, Some(error.to_string()))
            }
        };
        if let Ok(history) = &*lock(&history) {
            // History is best effort: a failed write must not affect the query.
            let _ = history.record(&NewHistoryEntry {
                connection_id: session.connection_id,
                sql: entry_sql,
                started_at,
                duration,
                rows,
                status,
                error,
            });
        }
    };
    let job = Job::start(session.conn, sql, limit, on_event, record);
    let job_id = state.next_id();
    lock(&state.jobs).insert(job_id, job);
    Ok(job_id)
}

/// Rows `offset..offset + limit` of a result, rendered as text, as an Arrow IPC stream.
/// Returns fewer rows when they have not been fetched yet.
#[tauri::command]
pub fn fetch_rows(
    state: State<'_, AppState>,
    job_id: u64,
    offset: usize,
    limit: usize,
) -> CommandResult<Response> {
    let job = state.job(job_id)?;
    job.request(offset.saturating_add(limit));
    let (schema, batches) = job
        .window(offset, limit)
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    Ok(Response::new(display::to_ipc(&schema, &batches)?))
}

#[tauri::command]
pub fn fetch_all(state: State<'_, AppState>, job_id: u64) -> CommandResult<()> {
    state.job(job_id)?.fetch_all();
    Ok(())
}

#[tauri::command]
pub async fn cancel_query(state: State<'_, AppState>, job_id: u64) -> CommandResult<()> {
    let job = state.job(job_id)?;
    job.cancel().await
}

/// Releases a result's buffers; an unfinished query is cancelled.
#[tauri::command]
pub fn close_query(state: State<'_, AppState>, job_id: u64) {
    if let Some(job) = lock(&state.jobs).remove(&job_id) {
        job.close();
    }
}

/// Schemas for the navigator. Navigator sessions are separate from editor sessions, so
/// browsing never waits behind a running query.
#[tauri::command]
pub async fn list_schemas(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<Vec<SchemaMeta>> {
    let session = state.session(session_id)?;
    Ok(session.conn.schemas().await?)
}

#[tauri::command]
pub async fn list_objects(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
) -> CommandResult<Vec<ObjectMeta>> {
    let session = state.session(session_id)?;
    Ok(session.conn.objects(&schema).await?)
}

#[tauri::command]
pub async fn table_details(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    table: String,
) -> CommandResult<TableDetails> {
    let session = state.session(session_id)?;
    Ok(session.conn.table_details(&schema, &table).await?)
}

/// A statement range in UTF-16 code units, the unit JavaScript strings use.
#[derive(Debug, Serialize)]
pub struct StatementSpan {
    start: usize,
    end: usize,
    read_only: bool,
}

#[derive(Debug, Serialize)]
pub struct SqlAnalysis {
    statements: Vec<StatementSpan>,
    /// Index into `statements` of the one to run for the cursor.
    at_cursor: Option<usize>,
}

/// Maps byte offsets of `src` to UTF-16 offsets and back.
struct Utf16Map {
    /// `(byte offset, utf16 offset)` at every char boundary, plus the end.
    points: Vec<(usize, usize)>,
}

impl Utf16Map {
    fn new(src: &str) -> Self {
        let mut points = Vec::with_capacity(src.len() + 1);
        let mut utf16 = 0;
        for (byte, c) in src.char_indices() {
            points.push((byte, utf16));
            utf16 += c.len_utf16();
        }
        points.push((src.len(), utf16));
        Self { points }
    }

    fn to_utf16(&self, byte: usize) -> usize {
        let i = self.points.partition_point(|&(b, _)| b < byte);
        self.points.get(i).map_or(0, |&(_, u)| u)
    }

    fn to_byte(&self, utf16: usize) -> usize {
        let i = self.points.partition_point(|&(_, u)| u < utf16);
        self.points
            .get(i)
            .map_or_else(|| self.points.last().map_or(0, |p| p.0), |&(b, _)| b)
    }
}

/// Splits `sql` into statements and finds the one under `cursor` (UTF-16 offset).
#[tauri::command]
pub fn analyze_sql(sql: String, driver: DriverKind, cursor: usize) -> SqlAnalysis {
    let dialect = Dialect::from(driver);
    let map = Utf16Map::new(&sql);
    let statements = platypus_sql::split(&sql, dialect);
    let at = platypus_sql::statement_at(&sql, dialect, map.to_byte(cursor));
    SqlAnalysis {
        at_cursor: at.and_then(|at| statements.iter().position(|s| *s == at)),
        statements: statements
            .iter()
            .map(|s| StatementSpan {
                start: map.to_utf16(s.start),
                end: map.to_utf16(s.end),
                read_only: platypus_sql::is_read_only(s.text(&sql), dialect),
            })
            .collect(),
    }
}

#[tauri::command]
pub fn search_history(
    state: State<'_, AppState>,
    connection_id: Option<Uuid>,
    text: Option<String>,
    before_id: Option<i64>,
    limit: usize,
) -> CommandResult<Vec<HistoryEntry>> {
    state.with_history(|h| {
        h.search(&HistoryQuery {
            connection_id,
            text,
            before_id,
            limit: limit.min(1000),
        })
    })
}

#[tauri::command]
pub fn delete_history(state: State<'_, AppState>, id: i64) -> CommandResult<()> {
    state.with_history(|h| h.delete(id))
}

/// Reads a script picked by the user in the open dialog.
#[tauri::command]
pub fn read_text_file(path: PathBuf) -> CommandResult<String> {
    std::fs::read_to_string(&path)
        .map_err(|e| CommandError::new("io", format!("cannot read {}: {e}", path.display())))
}

/// Writes a script to a path picked by the user in the save dialog.
#[tauri::command]
pub fn write_text_file(path: PathBuf, contents: String) -> CommandResult<()> {
    std::fs::write(&path, contents)
        .map_err(|e| CommandError::new("io", format!("cannot write {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_utf16_offsets() {
        // "é" is 2 bytes / 1 unit, "🦆" is 4 bytes / 2 units.
        let map = Utf16Map::new("aé🦆b");
        assert_eq!(map.to_utf16(0), 0);
        assert_eq!(map.to_utf16(3), 2);
        assert_eq!(map.to_utf16(7), 4);
        assert_eq!(map.to_utf16(8), 5);
        assert_eq!(map.to_byte(4), 7);
        assert_eq!(map.to_byte(99), 8);
    }

    #[test]
    fn analyzes_in_utf16_units() {
        let sql = "select '🦆';\nupdate t set x = 1".to_owned();
        let a = analyze_sql(sql, DriverKind::Postgres, 14);
        assert_eq!(a.statements.len(), 2);
        assert_eq!((a.statements[0].start, a.statements[0].end), (0, 11));
        assert!(a.statements[0].read_only);
        assert_eq!(a.statements[1].start, 13);
        assert!(!a.statements[1].read_only);
        assert_eq!(a.at_cursor, Some(1));
    }
}
