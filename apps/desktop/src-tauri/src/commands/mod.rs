//! Tauri commands: the whole API the web UI uses, one module per area. A feature adds its
//! own module and registers its commands in `lib.rs`; shared state lives here.

// Tauri injects `State` and deserialized arguments by value.
#![allow(clippy::needless_pass_by_value)]

pub mod admin;
pub mod agents;
pub mod analysis;
pub mod backup;
pub mod catalog;
pub mod cli;
pub mod connections;
pub mod dashboards;
pub mod data;
pub mod diagnostics;
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
pub mod profiles;
pub mod projects;
pub mod query;
pub mod querylog;
pub mod sample;
pub mod saved_results;
pub mod schedules;
pub mod schema;
pub mod schema_history;
pub mod scripts;
pub mod search;
pub mod settings;
pub mod snapshots;
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
use platypus_driver_redis::RedisDriver;
use platypus_driver_sqlite::SqliteDriver;
use platypus_store::{
    CatalogStore, ConnectionStore, HistoryStore, ImportedSecrets, SchemaHistoryStore,
    SettingsStore, VaultSecrets,
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
    /// Snapshots of schemas, for the schema history; `Err` when `schema-history.db` cannot be
    /// opened.
    schema_history: Mutex<Result<SchemaHistoryStore, String>>,
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
    /// A dialog's own session, beside the one the connection's tabs and navigator share.
    side: bool,
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
            side: false,
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
        schema_history: Result<SchemaHistoryStore, String>,
        settings: Result<SettingsStore, String>,
        scripts: Result<PathBuf, String>,
    ) -> Self {
        Self {
            store: Mutex::new(store),
            vault: Mutex::new(vault),
            vault_path,
            history: Arc::new(Mutex::new(history)),
            catalog: Mutex::new(catalog),
            schema_history: Mutex::new(schema_history),
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

    /// Runs `f` on the schema history; fails with `catalog` when `schema-history.db` could not
    /// be opened, so the history is unavailable while the rest of the app keeps working.
    pub(crate) fn with_schema_history<T>(
        &self,
        f: impl FnOnce(&mut SchemaHistoryStore) -> Result<T, platypus_store::StoreError>,
    ) -> CommandResult<T> {
        match &mut *lock(&self.schema_history) {
            Ok(history) => f(history).map_err(Into::into),
            Err(e) => Err(CommandError::new("catalog", e.clone())),
        }
    }

    /// Drops the notes and schema snapshots of connections that are no longer saved, e.g. when
    /// removing them failed earlier. Does nothing when the connections file could not be read.
    pub fn prune_catalog(&self) {
        let Ok(store) = &*lock(&self.store) else {
            return;
        };
        let ids: Vec<Uuid> = store.list().iter().map(|c| c.id).collect();
        if let Ok(catalog) = &mut *lock(&self.catalog) {
            let _ = catalog.retain_connections(&ids);
        }
        if let Ok(history) = &mut *lock(&self.schema_history) {
            let _ = history.retain_connections(&ids);
        }
    }

    /// Drops the notes of a connection that was just deleted; what this misses is pruned by
    /// `prune_catalog` at the next start.
    fn forget_catalog(&self, id: Uuid) {
        catalog::forget_connection(&mut lock(&self.catalog), id);
        if let Ok(history) = &mut *lock(&self.schema_history) {
            let _ = history.remove_connection(id);
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

    /// The setting `key`, if it is set and readable.
    pub(crate) fn setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        lock(&self.settings).as_ref().ok()?.get(key)
    }

    /// The sessions open, as agents see them through the bridge.
    pub(crate) fn open_sessions(&self) -> Vec<platypus_mcp::OpenSession> {
        let mut sessions: Vec<(u64, platypus_mcp::OpenSession)> = lock(&self.sessions)
            .iter()
            .map(|(id, s)| {
                let transaction = s.conn.transaction();
                let session = platypus_mcp::OpenSession {
                    connection: s.label.clone(),
                    database: Some(s.config.database.clone()).filter(|d| !d.is_empty()),
                    purpose: if s.side { "side" } else { "main" }.to_owned(),
                    manual_commit: !transaction.autocommit,
                    transaction_open: transaction.open,
                };
                (*id, session)
            })
            .collect();
        sessions.sort_by_key(|(id, _)| *id);
        sessions.into_iter().map(|(_, s)| s).collect()
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

    /// The dialect of the values and statements written on the session: its driver's, or
    /// that of the database behind the generic ODBC driver ([`Dialect::of_server`]).
    fn dialect(&self, id: u64) -> CommandResult<platypus_sql::Dialect> {
        let session = self.session(id)?;
        Ok(platypus_sql::Dialect::of_server(
            session.config.driver,
            &session.raw.server_info().product,
        ))
    }

    fn job(&self, id: u64) -> CommandResult<Arc<Job>> {
        lock(&self.jobs)
            .get(&id)
            .cloned()
            .ok_or_else(|| CommandError::not_found("query", id))
    }

    /// The name of the session's saved connection when `marked` says so of it. Read at
    /// each call, so a change to the settings applies to open sessions too.
    fn name_if(
        &self,
        session_id: u64,
        marked: impl Fn(&platypus_store::SavedConnection) -> bool,
    ) -> CommandResult<Option<String>> {
        let id = self.session(session_id)?.connection_id;
        Ok(match &*lock(&self.store) {
            Ok(store) => store.get(id).filter(|c| marked(c)).map(|c| c.name.clone()),
            // "Query with SQL" databases are not saved, and nothing else opens without it.
            Err(_) => None,
        })
    }

    /// The name of the session's saved connection when it is marked read-only.
    fn read_only_name(&self, session_id: u64) -> CommandResult<Option<String>> {
        self.name_if(session_id, |c| c.read_only)
    }

    /// Fails, saying `why` after the connection's name, when `restricted` says so of the
    /// session's connection.
    fn unrestricted(
        &self,
        session_id: u64,
        restricted: impl Fn(&platypus_store::Restrictions) -> bool,
        why: &str,
    ) -> CommandResult<()> {
        match self.name_if(session_id, |c| restricted(&c.restrictions))? {
            Some(name) => Err(CommandError::new("restricted", format!("{name} {why}"))),
            None => Ok(()),
        }
    }

    /// Fails when the session's connection keeps its data in: every command that writes
    /// it to a file or another database checks it here.
    fn exportable(&self, session_id: u64) -> CommandResult<()> {
        self.unrestricted(
            session_id,
            |r| r.no_export,
            "does not let its data be exported",
        )
    }

    /// Fails when the session's connection changes its rows only by SQL: the data editor,
    /// imports and generated rows check it here, with `writable`.
    fn editable(&self, session_id: u64) -> CommandResult<()> {
        self.writable(session_id)?;
        self.unrestricted(
            session_id,
            |r| r.no_data_editing,
            "changes its rows only by SQL",
        )
    }

    /// Fails when the session's connection is read-only: every command that writes checks
    /// it here, whatever the page offered.
    fn writable(&self, session_id: u64) -> CommandResult<()> {
        match self.read_only_name(session_id)? {
            Some(name) => Err(read_only_error(&name)),
            None => Ok(()),
        }
    }

    /// Fails when the session's connection is read-only and `sql` may change something:
    /// only statements that read run there.
    fn may_run(&self, session_id: u64, sql: &str) -> CommandResult<()> {
        let Some(name) = self.read_only_name(session_id)? else {
            return Ok(());
        };
        let driver = self.session(session_id)?.config.driver;
        if sql::only_reads(sql, driver) {
            Ok(())
        } else {
            Err(read_only_error(&name))
        }
    }
}

/// Runs a one-off change (a navigator drop, rename or truncate, a table editor save) so that
/// it is committed, or rolled back on error, before it returns: never left pending in a
/// transaction that nothing shows nor commits. A session in manual-commit mode runs it in
/// autocommit mode, then goes back; one with a transaction open refuses it.
pub(crate) async fn one_off<T, F>(conn: &Arc<dyn Connection>, run: F) -> CommandResult<T>
where
    F: std::future::Future<Output = CommandResult<T>>,
{
    let before = conn.transaction();
    if before.autocommit {
        return run.await;
    }
    if before.open {
        return Err(platypus_core::open_transaction_error().into());
    }
    conn.set_autocommit(true).await?;
    let result = run.await;
    // A statement that left a transaction open (it cannot be committed on its own) is undone.
    if conn.transaction().open {
        let _ = conn.rollback().await;
    }
    let restored = conn.set_autocommit(false).await;
    let value = result?;
    restored?;
    Ok(value)
}

fn read_only_error(name: &str) -> CommandError {
    CommandError::new(
        "read_only",
        format!("{name} is read-only: nothing that may change it runs there"),
    )
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
            DriverKind::Redis => RedisDriver.connect(&config).await,
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
    /// The commit and date the app is built from (`build.rs`), when built from a checkout.
    build: Option<&'static str>,
    os: &'static str,
    arch: &'static str,
    /// The developer tools port is open (`PLATYPUS_DEVTOOLS_PORT`), as for the end-to-end
    /// tests, which then get test hooks in the page.
    devtools: bool,
    /// A portable copy: everything it keeps is in the `data` folder next to it, and it
    /// updates by replacing its executable, not with the installer.
    portable: bool,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "PlatypusDB",
        version: platypus_core::VERSION,
        build: option_env!("PLATYPUS_BUILD"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        devtools: std::env::var_os("PLATYPUS_DEVTOOLS_PORT").is_some(),
        portable: platypus_mcp::profile::portable_dir().is_some(),
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
            Err(none.to_owned()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platypus_store::{MemorySecrets, PasswordUpdate, Restrictions, SavedConnection};

    use crate::commands::analysis::error_kind as kind;

    /// A state whose one saved SQLite connection, with `restrictions` and `read_only`, has
    /// a session open: its id.
    async fn session_of(restrictions: Restrictions, read_only: bool) -> (AppState, u64) {
        let dir = tempfile::tempdir().unwrap();
        let mut connections = ConnectionStore::open(
            dir.path().join("connections.toml"),
            Arc::new(MemorySecrets::default()),
        )
        .unwrap();
        let mut saved = SavedConnection::new("Ledger", DriverKind::Sqlite);
        saved.database = ":memory:".to_owned();
        saved.restrictions = restrictions;
        saved.read_only = read_only;
        let id = saved.id;
        let config = saved.config(None);
        connections.save(saved, PasswordUpdate::Keep).unwrap();
        let state = AppState::new(
            (Ok(connections), None),
            Err("no vault".to_owned()),
            Err("no history".to_owned()),
            Err("no catalog".to_owned()),
            Err("no schema history".to_owned()),
            Err("no settings".to_owned()),
            Err("no scripts".to_owned()),
        );
        let conn: Arc<dyn Connection> = Arc::from(connect(&config).await.unwrap());
        let session_id = state.next_id();
        lock(&state.sessions).insert(
            session_id,
            Session::new(id, Arc::clone(&conn), conn, config, "Ledger".to_owned()),
        );
        (state, session_id)
    }

    #[tokio::test]
    async fn restrictions_refuse_exports_and_edits() {
        let (state, sid) = session_of(Restrictions::default(), false).await;
        assert!(state.exportable(sid).is_ok() && state.editable(sid).is_ok());

        let no_export = Restrictions {
            no_export: true,
            ..Restrictions::default()
        };
        let (state, sid) = session_of(no_export, false).await;
        let e = serde_json::to_value(state.exportable(sid).unwrap_err()).unwrap();
        assert_eq!(e["kind"], "restricted");
        assert_eq!(e["message"], "Ledger does not let its data be exported");
        assert!(state.editable(sid).is_ok() && state.writable(sid).is_ok());

        let no_editing = Restrictions {
            no_data_editing: true,
            ..Restrictions::default()
        };
        let (state, sid) = session_of(no_editing, false).await;
        assert_eq!(kind(&state.editable(sid).unwrap_err()), "restricted");
        assert!(state.exportable(sid).is_ok() && state.writable(sid).is_ok());

        // Read-only refuses edits as it did, before the restriction.
        let (state, sid) = session_of(no_editing, true).await;
        assert_eq!(kind(&state.editable(sid).unwrap_err()), "read_only");
    }
}
