//! Tauri commands: the whole API the web UI uses, one module per area. A feature adds its
//! own module and registers its commands in `lib.rs`; shared state lives here.

// Tauri injects `State` and deserialized arguments by value.
#![allow(clippy::needless_pass_by_value)]

pub mod agents;
pub mod analysis;
pub mod catalog;
pub mod cli;
pub mod connections;
pub mod data;
pub mod drivers;
pub mod explain;
pub mod export;
pub mod files;
pub mod geometry;
pub mod grid;
pub mod health;
pub mod history;
pub mod meta;
pub mod objects;
pub mod projects;
pub mod query;
pub mod querylog;
pub mod schema;
pub mod scripts;
pub mod search;
pub mod settings;
pub mod sql;
pub mod summary;
pub mod transactions;
pub mod transfer;
pub mod vault;
pub mod vendor;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use platypus_core::{Connection, ConnectionConfig, Driver, DriverKind, Error};
use platypus_driver_adbc::AdbcDriver;
use platypus_driver_clickhouse::ClickHouseDriver;
use platypus_driver_duckdb::DuckDbDriver;
use platypus_driver_libsql::LibsqlDriver;
use platypus_driver_mongo::MongoDriver;
use platypus_driver_mssql::MssqlDriver;
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_odbc::OdbcDriver;
use platypus_driver_oracle::OracleDriver;
use platypus_driver_pg::PgDriver;
use platypus_driver_sqlite::SqliteDriver;
use platypus_store::{
    CatalogStore, ConnectionStore, HistoryStore, ImportedSecrets, SettingsStore, VaultSecrets,
};
use serde::Serialize;
use uuid::Uuid;

use crate::error::{CommandError, CommandResult};
use crate::export::Export;
use crate::jobs::Job;
use crate::querylog::{Logged, QueryLog};

#[derive(Debug)]
pub struct AppState {
    /// `Err` when the connections file exists but cannot be read; shown instead of wiping it.
    store: Mutex<Result<ConnectionStore, String>>,
    /// The encrypted passwords file, when a master password is set; the store reads it.
    vault: Mutex<Option<Arc<VaultSecrets>>>,
    /// Where a vault goes when the master password is turned on.
    vault_path: Result<PathBuf, String>,
    /// Shared with job tasks, which record outcomes as they happen.
    history: Arc<Mutex<Result<HistoryStore, String>>>,
    /// Notes and tags on tables and columns; `Err` when `catalog.db` cannot be opened.
    catalog: Mutex<Result<CatalogStore, String>>,
    settings: Mutex<Result<SettingsStore, String>>,
    /// The folder of the saved scripts, see `scripts`.
    scripts: Result<PathBuf, String>,
    sessions: Mutex<HashMap<u64, Session>>,
    jobs: Mutex<HashMap<u64, Arc<Job>>>,
    /// "Query with SQL": the in-memory databases of the temporary connections, by their id.
    result_dbs: Mutex<HashMap<Uuid, analysis::sql::ResultDb>>,
    exports: Mutex<HashMap<u64, Arc<Export>>>,
    /// Scripts running in a command-line client, by the editor's run id: sending stops one.
    clis: Mutex<HashMap<u64, tokio::sync::oneshot::Sender<()>>>,
    /// "Compare with previous run": the rows of each session's previous run.
    previous_runs: Mutex<HashMap<u64, analysis::diff::PreviousRun>>,
    /// Documentation exports running, by id: cancelling one stops it before the file is written.
    docs: Mutex<HashMap<u64, tokio_util::sync::CancellationToken>>,
    /// The passwords of the last file read for an import, by connection id: they stay here,
    /// out of the page, until the import is saved.
    import_secrets: Mutex<HashMap<Uuid, ImportedSecrets>>,
    /// What the query manager shows.
    query_log: Arc<QueryLog>,
    next_id: AtomicU64,
}

#[derive(Debug, Clone)]
struct Session {
    connection_id: Uuid,
    conn: Arc<dyn Connection>,
    /// How the session was opened, password included: exports open their own connection.
    config: ConnectionConfig,
    /// The connection's name in the query log.
    label: String,
    /// `conn` without the query log, for keep-alives.
    raw: Arc<dyn Connection>,
    /// When the session was last given work, shared by the clones of the session.
    last_used: Arc<Mutex<Instant>>,
}

impl Session {
    fn new(
        connection_id: Uuid,
        raw: Arc<dyn Connection>,
        conn: Arc<dyn Connection>,
        config: ConnectionConfig,
        label: String,
    ) -> Self {
        Self {
            connection_id,
            conn,
            config,
            label,
            raw,
            last_used: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// How long the session sat idle; it is in use from now.
    fn idle(&self) -> Duration {
        let mut last = lock(&self.last_used);
        let idle = last.elapsed();
        *last = Instant::now();
        idle
    }
}

/// A session idle this long is asked whether the server still holds it before it gets
/// more work (see `Connection::check_alive`): statements of a script that follow one
/// another pay no round trip, a statement run by hand does.
const IDLE_CHECK: Duration = Duration::from_secs(1);

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl AppState {
    pub fn new(
        (store, vault): (Result<ConnectionStore, String>, Option<Arc<VaultSecrets>>),
        vault_path: Result<PathBuf, String>,
        history: Result<HistoryStore, String>,
        catalog: Result<CatalogStore, String>,
        settings: Result<SettingsStore, String>,
        scripts: Result<PathBuf, String>,
    ) -> Self {
        Self {
            store: Mutex::new(store),
            vault: Mutex::new(vault),
            vault_path,
            history: Arc::new(Mutex::new(history)),
            catalog: Mutex::new(catalog),
            settings: Mutex::new(settings),
            scripts,
            sessions: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
            result_dbs: Mutex::new(HashMap::new()),
            exports: Mutex::new(HashMap::new()),
            clis: Mutex::new(HashMap::new()),
            previous_runs: Mutex::new(HashMap::new()),
            docs: Mutex::new(HashMap::new()),
            import_secrets: Mutex::new(HashMap::new()),
            query_log: Arc::default(),
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

    /// Runs `f` on the catalog; fails with `catalog` when `catalog.db` could not be opened, so
    /// the notes are unavailable while the rest of the app keeps working.
    fn with_catalog<T>(
        &self,
        f: impl FnOnce(&mut CatalogStore) -> Result<T, platypus_store::StoreError>,
    ) -> CommandResult<T> {
        match &mut *lock(&self.catalog) {
            Ok(catalog) => f(catalog).map_err(Into::into),
            Err(e) => Err(CommandError::new("catalog", e.clone())),
        }
    }

    /// Drops the notes of connections that are no longer saved, e.g. when removing them failed
    /// earlier. Does nothing when the connections file could not be read.
    pub fn prune_catalog(&self) {
        let Ok(store) = &*lock(&self.store) else {
            return;
        };
        let ids: Vec<Uuid> = store.list().iter().map(|c| c.id).collect();
        if let Ok(catalog) = &mut *lock(&self.catalog) {
            let _ = catalog.retain_connections(&ids);
        }
    }

    /// Drops the notes of a connection that was just deleted; what this misses is pruned by
    /// `prune_catalog` at the next start.
    fn forget_catalog(&self, id: Uuid) {
        catalog::forget_connection(&mut lock(&self.catalog), id);
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

    /// The session's connection, reopened first if the server dropped it: after a while
    /// idle, the driver is asked (a server may drop a session without the driver noticing
    /// until the next statement, which would fail). Nothing that was sent is run again.
    /// Manual-commit mode carries over; a transaction that was open is lost, which is
    /// reported as an error instead of running the statement outside it.
    async fn live(&self, id: u64) -> CommandResult<Arc<dyn Connection>> {
        let session = self.session(id)?;
        let idle = session.idle();
        let alive =
            !session.conn.is_closed() && (idle < IDLE_CHECK || session.raw.check_alive().await);
        if alive {
            return Ok(session.conn);
        }
        let previous = session.conn.transaction();
        let raw: Arc<dyn Connection> = Arc::from(connect(&session.config).await?);
        let conn = Logged::wrap(Arc::clone(&raw), &self.query_log, &session.label);
        if !previous.autocommit {
            conn.set_autocommit(false).await?;
        }
        if let Some(s) = lock(&self.sessions).get_mut(&id) {
            s.conn = Arc::clone(&conn);
            s.raw = raw;
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
            DriverKind::DuckDb => DuckDbDriver.connect(&config).await,
            DriverKind::Libsql => LibsqlDriver.connect(&config).await,
            DriverKind::SqlServer => MssqlDriver.connect(&config).await,
            DriverKind::ClickHouse => ClickHouseDriver.connect(&config).await,
            DriverKind::Mongo => MongoDriver.connect(&config).await,
            DriverKind::Snowflake
            | DriverKind::BigQuery
            | DriverKind::Databricks
            | DriverKind::Trino
            | DriverKind::Adbc => AdbcDriver::shared()?.connect(&config).await,
            DriverKind::Oracle => OracleDriver::shared()?.connect(&config).await,
            DriverKind::Db2 | DriverKind::Odbc => OdbcDriver.connect(&config).await,
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
    /// The developer tools port is open (`PLATYPUS_DEVTOOLS_PORT`), as for the end-to-end
    /// tests, which then get test hooks in the page.
    devtools: bool,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "PlatypusDB",
        version: platypus_core::VERSION,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        devtools: std::env::var_os("PLATYPUS_DEVTOOLS_PORT").is_some(),
    }
}

#[cfg(test)]
impl AppState {
    /// A state with nothing opened (no connections file, history, catalog, settings or scripts),
    /// for commands that only use sessions and results.
    pub(crate) fn for_tests() -> Self {
        let none = "not opened in tests";
        Self::new(
            (Err(none.to_owned()), None),
            Err(none.to_owned()),
            Err(none.to_owned()),
            Err(none.to_owned()),
            Err(none.to_owned()),
            Err(none.to_owned()),
        )
    }
}
