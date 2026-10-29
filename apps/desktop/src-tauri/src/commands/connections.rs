//! Saved connections and the sessions opened from them.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use platypus_core::{
    Capabilities, Connection, ConnectionConfig, DriverKind, ParseNote, ParsedConnection, Secret,
    ServerInfo, SslMode,
};
use platypus_sql::Dialect;
use platypus_store::{
    ConnectionStore, ConnectionType, ImportFile, ImportRequest, ImportSource, ImportedProject,
    NetworkProfile, PasswordUpdate, SavedConnection, SavedSsh,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use super::{AppState, Session, connect, lock};
use crate::error::{CommandError, CommandResult};
use crate::querylog::Logged;

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

/// The form's fields from a pasted URL or connection string. No `Debug`: it holds the
/// password, which goes to the form's password field only.
#[derive(Serialize)]
pub struct ParsedForm {
    driver: DriverKind,
    host: String,
    port: Option<u16>,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: SslMode,
    ssl_root_cert: Option<PathBuf>,
    ssl_client_cert: Option<PathBuf>,
    ssl_client_key: Option<PathBuf>,
    options: BTreeMap<String, String>,
    /// What the user should know: logins the app does not do, parameters left out.
    notes: Vec<ParseNote>,
}

/// Reads a URL (`postgres://…`), a JDBC URL or an ADO.NET / ODBC connection string into
/// the fields of the connection form; `driver` is the form's, when it has one (see
/// [`ConnectionConfig::parse_any_for`]). Nothing is logged: the input may hold a password.
#[tauri::command]
pub fn parse_connection_url(input: &str, driver: Option<DriverKind>) -> CommandResult<ParsedForm> {
    let ParsedConnection { config, notes } = ConnectionConfig::parse_any_for(input, driver)?;
    Ok(ParsedForm {
        driver: config.driver,
        host: config.host,
        port: config.port,
        database: config.database,
        user: config.user,
        password: config.password.map(|p| p.expose().to_owned()),
        ssl_mode: config.ssl_mode,
        ssl_root_cert: config.ssl_root_cert,
        ssl_client_cert: config.ssl_client_cert,
        ssl_client_key: config.ssl_client_key,
        options: config.options,
        notes,
    })
}

/// Whether a password is saved for the connection, for the form's "(saved)" placeholder.
#[tauri::command]
pub fn connection_has_password(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> CommandResult<bool> {
    state.with_store(|s| s.has_password(connection_id))
}

/// Saves the password typed at the prompt of a connection that keeps it in the keychain
/// but had none saved.
#[tauri::command]
pub fn remember_password(
    state: State<'_, AppState>,
    connection_id: Uuid,
    password: String,
) -> CommandResult<()> {
    state.with_store(|s| s.set_password(connection_id, &Secret::new(password)))
}

/// A connection saved as a copy of another, and everything it joined.
#[derive(Serialize)]
pub struct Duplicated {
    connection: SavedConnection,
    workspace: super::projects::Workspace,
}

/// Saves a copy of a connection with its secrets, next to it (see
/// `ConnectionStore::duplicate`).
#[tauri::command]
pub fn duplicate_connection(state: State<'_, AppState>, id: Uuid) -> CommandResult<Duplicated> {
    state.with_store(|s| {
        let connection = s.duplicate(id)?;
        Ok(Duplicated {
            connection,
            workspace: super::projects::Workspace::of(s),
        })
    })
}

/// The URL of a saved connection, without its password.
#[tauri::command]
pub fn connection_url(state: State<'_, AppState>, connection_id: Uuid) -> CommandResult<String> {
    state.with_store(|s| s.url_of(connection_id))
}

/// The URL of the connection as the form has it, without a password; `None` for the
/// drivers that have no URL. Nothing is saved or read from the store.
#[tauri::command]
pub fn form_connection_url(connection: SavedConnection) -> Option<String> {
    connection.config(None).to_url()
}

/// SSH passwords and passphrases typed in the form, by the `secret_id` of their server.
pub type SshSecrets = HashMap<Uuid, PasswordInput>;

/// The SSH settings of `ssh` with the form's secrets, else the saved ones.
fn ssh_config(
    store: &ConnectionStore,
    ssh: &SavedSsh,
    typed: &SshSecrets,
) -> platypus_core::SshConfig {
    ssh.config(&|id| match typed.get(&id) {
        Some(PasswordInput::Set(p)) => Some(Secret::new(p.clone())),
        Some(PasswordInput::Clear) => None,
        Some(PasswordInput::Keep) | None => store.ssh_secret(id).ok().flatten(),
    })
}

/// The settings to connect with: the network profile's SSH tunnel and SSL in place of the
/// connection's own, and the bootstrap script split into statements.
fn config_of(
    store: &ConnectionStore,
    saved: &SavedConnection,
    password: Option<Secret>,
    ssh: &SshSecrets,
) -> ConnectionConfig {
    let saved = store.effective(saved);
    let mut config = saved.config(password);
    config.ssh = saved.ssh.as_ref().map(|s| ssh_config(store, s, ssh));
    let script = &saved.init.statements;
    config.init.statements = if saved.driver == DriverKind::Mongo {
        platypus_driver_mongo::shell::split(script)
            .into_iter()
            .map(|r| script[r].to_owned())
            .collect()
    } else {
        platypus_sql::split(script, Dialect::from(saved.driver))
            .iter()
            .map(|s| s.text(script).to_owned())
            .collect()
    };
    config
}

/// The password from the connection's command or `.pgpass`, if it uses one of them. A
/// password command may take a while: it runs off the async runtime.
async fn external_password(saved: &SavedConnection) -> CommandResult<Option<Secret>> {
    if saved.password_command.is_none() && !saved.pgpass {
        return Ok(None);
    }
    let saved = saved.clone();
    let secret =
        tauri::async_runtime::spawn_blocking(move || platypus_store::external_password(&saved))
            .await
            .map_err(|e| CommandError::new("other", e.to_string()))??;
    Ok(secret)
}

/// Updates the secrets of an SSH chain that changed from `old` to `new`: servers that left
/// it take theirs with them, typed ones are set or cleared.
fn update_ssh_secrets(
    store: &ConnectionStore,
    old: Option<&SavedSsh>,
    new: Option<&SavedSsh>,
    typed: SshSecrets,
) -> platypus_store::Result<()> {
    let kept: Vec<Uuid> = new
        .into_iter()
        .flat_map(SavedSsh::chain)
        .map(|h| h.secret_id)
        .collect();
    for h in old.into_iter().flat_map(SavedSsh::chain) {
        if !kept.contains(&h.secret_id) {
            store.set_ssh_secret(h.secret_id, None)?;
        }
    }
    for (id, input) in typed {
        match input {
            PasswordInput::Set(p) => store.set_ssh_secret(id, Some(&Secret::new(p)))?,
            PasswordInput::Clear => store.set_ssh_secret(id, None)?,
            PasswordInput::Keep => {}
        }
    }
    Ok(())
}

#[tauri::command]
pub fn save_connection(
    state: State<'_, AppState>,
    connection: SavedConnection,
    password: PasswordInput,
    ssh_secrets: Option<SshSecrets>,
    projects: Option<Vec<Uuid>>,
) -> CommandResult<Vec<SavedConnection>> {
    let id = connection.id;
    let update = match password {
        PasswordInput::Keep => PasswordUpdate::Keep,
        PasswordInput::Set(p) => PasswordUpdate::Set(Secret::new(p)),
        PasswordInput::Clear => PasswordUpdate::Clear,
    };
    state.with_store(|s| {
        let old = s.get(connection.id).and_then(|c| c.ssh.clone());
        let new = connection.ssh.clone();
        s.save(connection, update)?;
        update_ssh_secrets(
            s,
            old.as_ref(),
            new.as_ref(),
            ssh_secrets.unwrap_or_default(),
        )?;
        // `None` leaves the connection in the projects it is in.
        if let Some(projects) = projects {
            s.set_projects_of(id, &projects)?;
        }
        Ok(s.list().to_vec())
    })
}

/// Connection files of other tools found where they keep them.
#[tauri::command]
pub fn detect_imports() -> Vec<ImportFile> {
    platypus_store::detect_imports()
}

/// The projects and connections of another tool's file, for the user to pick from;
/// nothing is saved. The passwords found stay in the backend for `save_imported`.
#[tauri::command]
pub fn read_import(
    state: State<'_, AppState>,
    source: ImportSource,
    path: PathBuf,
) -> CommandResult<Vec<ImportedProject>> {
    let projects = platypus_store::import_connections(source, &path)?;
    *lock(&state.import_secrets) = projects
        .iter()
        .flat_map(|p| p.secrets.iter().map(|(id, s)| (*id, s.clone())))
        .collect();
    Ok(projects)
}

/// Saves what was picked from an import, all or nothing. With `request.passwords`, the
/// passwords read with the file are saved too; the other connections ask on first use.
#[tauri::command]
pub fn save_imported(
    state: State<'_, AppState>,
    mut request: ImportRequest,
) -> CommandResult<super::projects::Workspace> {
    {
        let read = lock(&state.import_secrets);
        for project in request.targets.iter_mut().map(|t| &mut t.project) {
            project.secrets = project
                .connections
                .iter()
                .filter_map(|c| Some((c.id, read.get(&c.id)?.clone())))
                .collect();
        }
    }
    let workspace = state.with_store(|s| {
        s.apply_import(request)?;
        Ok(super::projects::Workspace::of(s))
    })?;
    lock(&state.import_secrets).clear();
    Ok(workspace)
}

/// Drops the passwords `read_import` keeps, when the import dialog closes without saving.
#[tauri::command]
pub fn forget_import(state: State<'_, AppState>) {
    lock(&state.import_secrets).clear();
}

#[tauri::command]
pub fn delete_connection(
    state: State<'_, AppState>,
    id: Uuid,
) -> CommandResult<Vec<SavedConnection>> {
    let list = state.with_store(|s| {
        s.delete(id)?;
        Ok(s.list().to_vec())
    })?;
    // Leftovers (a failed delete here) are pruned at the next start.
    state.forget_catalog(id);
    Ok(list)
}

/// Sets the logical key the data editor uses for a table without a primary key; no
/// columns remove it.
#[tauri::command]
pub fn set_virtual_key(
    state: State<'_, AppState>,
    connection_id: Uuid,
    schema: String,
    table: String,
    columns: Vec<String>,
) -> CommandResult<Vec<SavedConnection>> {
    state.with_store(|s| {
        s.set_virtual_key(connection_id, &schema, &table, columns)?;
        Ok(s.list().to_vec())
    })
}

/// Connects with the form's current values, without saving them.
#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    connection: SavedConnection,
    password: PasswordInput,
    ssh_secrets: Option<SshSecrets>,
) -> CommandResult<ServerInfo> {
    let ssh = ssh_secrets.unwrap_or_default();
    let secret = match password {
        PasswordInput::Set(p) => Some(Secret::new(p)),
        PasswordInput::Clear => None,
        PasswordInput::Keep => match external_password(&connection).await? {
            Some(secret) => Some(secret),
            // A connection that was never saved has nothing stored yet.
            None => state.with_store(|s| match s.get(connection.id) {
                Some(_) => s.password(connection.id),
                None => Ok(None),
            })?,
        },
    };
    let config = state
        .with_store(|s| Ok(config_of(s, &connection, secret, &ssh)))?
        .with_env()?;
    if !config.driver.is_embedded() {
        config.check_host()?;
    }
    let conn = connect(&config).await?;
    Ok(conn.server_info().clone())
}

/// Whether a failed login is about the credentials, so that asking for the password again
/// may help. The drivers word it their own way.
fn login_refused(e: &CommandError) -> bool {
    let text = e.message().to_ascii_lowercase();
    [
        "password",
        "authenticat",
        "access denied",
        "login failed",
        "credentials",
    ]
    .iter()
    .any(|word| text.contains(word))
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    session_id: u64,
    server: ServerInfo,
    /// What the driver supports, so the UI can hide actions it would reject.
    capabilities: Capabilities,
    /// The oldest version tested, when the server is older: the UI says some features may
    /// not work.
    minimum: Option<String>,
}

/// Opens a session for a saved connection. `password` overrides the stored one, `database`
/// the saved database (another database of the same server). A `side` session (the
/// navigator's) stays in autocommit mode on production too: it has no Commit button, and its
/// one-off changes commit once their dialog is confirmed.
#[tauri::command]
pub async fn open_session(
    state: State<'_, AppState>,
    connection_id: Uuid,
    password: Option<String>,
    database: Option<String>,
    side: Option<bool>,
) -> CommandResult<SessionInfo> {
    // "Query with SQL" reaches its in-memory database through a connection that is not saved.
    if let Some((config, label)) = state.result_db_config(connection_id) {
        return start_session(&state, connection_id, config, label, (false, false)).await;
    }
    let typed = password.is_some();
    let SavedConfig {
        config,
        label,
        none_saved,
        production,
        ..
    } = saved_config(&state, connection_id, password, database).await?;
    let side = side == Some(true);
    let production = production && !side;
    // `auth`: the UI asks for the password (again), with the server's answer.
    start_session(&state, connection_id, config, label, (production, side))
        .await
        .map_err(|e| {
            if (typed || none_saved) && login_refused(&e) {
                e.with_kind("auth")
            } else {
                e
            }
        })
}

/// A saved connection ready to connect: its settings with the password, and its name.
pub(crate) struct SavedConfig {
    pub config: ConnectionConfig,
    pub label: String,
    /// The password should be in the keychain, and is not (never typed, deleted, another
    /// machine).
    pub none_saved: bool,
    /// A production connection.
    pub production: bool,
    /// Nothing that may change data runs on it.
    pub read_only: bool,
    /// What it does not let happen, e.g. its data leave it.
    pub restrictions: platypus_store::Restrictions,
}

/// The settings of saved connection `connection_id`, on `database` when given, with
/// `password`, or the one its password command prints, or the saved one; environment
/// variables replaced.
pub(crate) async fn saved_config(
    state: &AppState,
    connection_id: Uuid,
    password: Option<String>,
    database: Option<String>,
) -> CommandResult<SavedConfig> {
    let saved = state.with_store(|s| {
        s.get(connection_id)
            .cloned()
            .ok_or(platypus_store::StoreError::NotFound(connection_id))
    })?;
    // A password command may take a while: it runs without holding the store.
    let secret = match password {
        Some(p) => Some(Secret::new(p)),
        None => match external_password(&saved).await? {
            Some(secret) => Some(secret),
            None => state.with_store(|s| s.password(connection_id))?,
        },
    };
    let none_saved = secret.is_none()
        && !saved.driver.is_embedded()
        && saved.save_password
        && saved.password_command.is_none()
        && !saved.pgpass;
    let mut config = state
        .with_store(|s| Ok(config_of(s, &saved, secret, &SshSecrets::new())))?
        .with_env()?;
    if !config.driver.is_embedded() {
        config.check_host()?;
    }
    let label = match &database {
        Some(database) => format!("{} · {database}", saved.name),
        None => saved.name.clone(),
    };
    if let Some(database) = database {
        config.database = database;
    }
    Ok(SavedConfig {
        config,
        label,
        none_saved,
        production: saved.connection_type == Some(ConnectionType::Prod),
        read_only: saved.read_only,
        restrictions: saved.restrictions,
    })
}

/// Connects with `config` and keeps the session. Production starts in manual commit, so that
/// nothing changes until Commit.
async fn start_session(
    state: &AppState,
    connection_id: Uuid,
    config: ConnectionConfig,
    label: String,
    (production, side): (bool, bool),
) -> CommandResult<SessionInfo> {
    let raw: Arc<dyn Connection> = Arc::from(connect(&config).await?);
    let conn = Logged::wrap(Arc::clone(&raw), &state.query_log, &label);
    if production && conn.capabilities().transactions {
        conn.set_autocommit(false).await?;
    }
    let server = conn.server_info().clone();
    let minimum = server.below_minimum().map(|v| v.to_string());
    let capabilities = conn.capabilities();
    let session_id = state.next_id();
    let mut session = Session::new(connection_id, raw, conn, config, label);
    session.side = side;
    lock(&state.sessions).insert(session_id, session);
    Ok(SessionInfo {
        session_id,
        server,
        capabilities,
        minimum,
    })
}

#[tauri::command]
pub fn list_network_profiles(state: State<'_, AppState>) -> CommandResult<Vec<NetworkProfile>> {
    state.with_store(|s| Ok(s.network_profiles().to_vec()))
}

/// Saves a network profile and its SSH secrets; returns every profile.
#[tauri::command]
pub fn save_network_profile(
    state: State<'_, AppState>,
    profile: NetworkProfile,
    ssh_secrets: Option<SshSecrets>,
) -> CommandResult<Vec<NetworkProfile>> {
    state.with_store(|s| {
        let old = s
            .network_profiles()
            .iter()
            .find(|p| p.id == profile.id)
            .and_then(|p| p.ssh.clone());
        let new = profile.ssh.clone();
        s.save_network_profile(profile)?;
        update_ssh_secrets(
            s,
            old.as_ref(),
            new.as_ref(),
            ssh_secrets.unwrap_or_default(),
        )?;
        Ok(s.network_profiles().to_vec())
    })
}

#[tauri::command]
pub fn delete_network_profile(
    state: State<'_, AppState>,
    id: Uuid,
) -> CommandResult<Vec<NetworkProfile>> {
    state.with_store(|s| {
        s.delete_network_profile(id)?;
        Ok(s.network_profiles().to_vec())
    })
}

#[tauri::command]
pub fn close_session(state: State<'_, AppState>, session_id: u64) {
    lock(&state.sessions).remove(&session_id);
    super::analysis::diff::forget(&state, session_id);
}
