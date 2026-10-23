//! Tauri commands: the whole API the web UI uses.

// Tauri injects `State` and deserialized arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use platypus_core::{Connection, ConnectionConfig, Driver, DriverKind, Error, Secret, ServerInfo};
use platypus_driver_pg::PgDriver;
use platypus_store::{ConnectionStore, PasswordUpdate, SavedConnection};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::{Channel, Response};
use uuid::Uuid;

use crate::display;
use crate::error::{CommandError, CommandResult};
use crate::jobs::{Job, QueryEvent};

#[derive(Debug)]
pub struct AppState {
    /// `Err` when the connections file exists but cannot be read; shown instead of wiping it.
    store: Mutex<Result<ConnectionStore, String>>,
    sessions: Mutex<HashMap<u64, Session>>,
    jobs: Mutex<HashMap<u64, Arc<Job>>>,
    next_id: AtomicU64,
}

#[derive(Debug, Clone)]
struct Session {
    conn: Arc<dyn Connection>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl AppState {
    pub fn new(store: Result<ConnectionStore, String>) -> Self {
        Self {
            store: Mutex::new(store),
            sessions: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
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
        other => Err(Error::Unsupported(format!(
            "{other:?} driver (lands in M4)"
        ))),
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
    lock(&state.sessions).insert(session_id, Session { conn });
    Ok(SessionInfo { session_id, server })
}

#[tauri::command]
pub fn close_session(state: State<'_, AppState>, session_id: u64) {
    lock(&state.sessions).remove(&session_id);
}

/// Starts a statement and returns its job id at once; progress arrives on `on_event`.
#[tauri::command]
pub fn run_query(
    state: State<'_, AppState>,
    session_id: u64,
    sql: String,
    on_event: Channel<QueryEvent>,
) -> CommandResult<u64> {
    let session = state.session(session_id)?;
    let job = Job::start(session.conn, sql, on_event);
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
