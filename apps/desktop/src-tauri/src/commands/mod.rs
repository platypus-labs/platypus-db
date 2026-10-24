//! Tauri commands: the whole API the web UI uses, one module per area. A feature adds its
//! own module and registers its commands in `lib.rs`; shared state lives here.

// Tauri injects `State` and deserialized arguments by value.
#![allow(clippy::needless_pass_by_value)]

pub mod connections;
pub mod data;
pub mod export;
pub mod files;
pub mod grid;
pub mod history;
pub mod meta;
pub mod objects;
pub mod query;
pub mod settings;
pub mod sql;
pub mod summary;
pub mod transactions;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use platypus_core::{Connection, ConnectionConfig, Driver, DriverKind, Error};
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_pg::PgDriver;
use platypus_driver_sqlite::SqliteDriver;
use platypus_store::{ConnectionStore, HistoryStore, SettingsStore};
use serde::Serialize;
use uuid::Uuid;

use crate::error::{CommandError, CommandResult};
use crate::export::Export;
use crate::jobs::Job;

#[derive(Debug)]
pub struct AppState {
    /// `Err` when the connections file exists but cannot be read; shown instead of wiping it.
    store: Mutex<Result<ConnectionStore, String>>,
    /// Shared with job tasks, which record outcomes as they happen.
    history: Arc<Mutex<Result<HistoryStore, String>>>,
    settings: Mutex<Result<SettingsStore, String>>,
    sessions: Mutex<HashMap<u64, Session>>,
    jobs: Mutex<HashMap<u64, Arc<Job>>>,
    exports: Mutex<HashMap<u64, Arc<Export>>>,
    next_id: AtomicU64,
}

#[derive(Debug, Clone)]
struct Session {
    connection_id: Uuid,
    conn: Arc<dyn Connection>,
    /// How the session was opened, password included: exports open their own connection.
    config: ConnectionConfig,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl AppState {
    pub fn new(
        store: Result<ConnectionStore, String>,
        history: Result<HistoryStore, String>,
        settings: Result<SettingsStore, String>,
    ) -> Self {
        Self {
            store: Mutex::new(store),
            history: Arc::new(Mutex::new(history)),
            settings: Mutex::new(settings),
            sessions: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
            exports: Mutex::new(HashMap::new()),
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

    fn with_settings<T>(
        &self,
        f: impl FnOnce(&mut SettingsStore) -> Result<T, platypus_store::StoreError>,
    ) -> CommandResult<T> {
        match &mut *lock(&self.settings) {
            Ok(settings) => f(settings).map_err(Into::into),
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

    /// The session's connection, reopened first if the server dropped it. Manual-commit
    /// mode carries over; a transaction that was open is lost, which is reported as an
    /// error instead of running the statement outside it.
    async fn live(&self, id: u64) -> CommandResult<Arc<dyn Connection>> {
        let session = self.session(id)?;
        if !session.conn.is_closed() {
            return Ok(session.conn);
        }
        let previous = session.conn.transaction();
        let conn: Arc<dyn Connection> = Arc::from(connect(&session.config).await?);
        if !previous.autocommit {
            conn.set_autocommit(false).await?;
        }
        if let Some(s) = lock(&self.sessions).get_mut(&id) {
            s.conn = Arc::clone(&conn);
        }
        if previous.open {
            return Err(CommandError::new(
                "reconnected",
                "The connection was lost and has been reopened; the open transaction was rolled back.",
            ));
        }
        Ok(conn)
    }

    fn job(&self, id: u64) -> CommandResult<Arc<Job>> {
        lock(&self.jobs)
            .get(&id)
            .cloned()
            .ok_or_else(|| CommandError::not_found("query", id))
    }
}

/// Opens a connection, through its SSH tunnel when it has one.
pub(crate) async fn connect(config: &ConnectionConfig) -> Result<Box<dyn Connection>, Error> {
    platypus_net::connect(config, None, |config| async move {
        match config.driver {
            DriverKind::Postgres => PgDriver.connect(&config).await,
            DriverKind::MySql => MySqlDriver.connect(&config).await,
            DriverKind::Sqlite => SqliteDriver.connect(&config).await,
        }
    })
    .await
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
