//! Saved connections and the sessions opened from them.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use platypus_core::{
    Capabilities, Connection, ConnectionConfig, DriverKind, Error, ParseNote, ParsedConnection,
    Secret, ServerInfo, SslMode,
};
use platypus_plugins::{HOOKS, HookEvent};
use platypus_sql::Dialect;
use platypus_store::{
    ConnectionStore, ConnectionType, ImportFile, ImportRequest, ImportSource, ImportedProject,
    NetworkProfile, PasswordUpdate, SavedConnection, SavedSsh,
};
use platypus_sync::init_approval::{approval_key, init_hash, needs_approval};
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
    // A URL copied from a sentence or a `.env` line carries its `;` or `,`.
    let input = platypus_store::trim_url_tail(input);
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

/// Whether this build can sign in to PostgreSQL with Kerberos.
#[tauri::command]
#[must_use]
pub fn kerberos_available() -> bool {
    platypus_driver_pg::KERBEROS_AVAILABLE
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
    let mut saved = store.effective(saved);
    platypus_store::tidy_address(&mut saved);
    let mut config = saved.config(password);
    config.ssh = saved.ssh.as_ref().map(|s| ssh_config(store, s, ssh));
    let script = &saved.init.statements;
    config.init.statements = if saved.driver == DriverKind::Mongo {
        platypus_driver_mongo::shell::split(script)
            .into_iter()
            .map(|r| script[r].to_owned())
            .collect()
    } else if saved.driver == DriverKind::Redis {
        platypus_driver_redis::command::split(script)
            .into_iter()
            .map(|r| script[r].to_owned())
            .collect()
    } else if saved.driver == DriverKind::Elasticsearch {
        platypus_driver_search::command::split(script)
            .into_iter()
            .map(|r| script[r].to_owned())
            .collect()
    } else if saved.driver == DriverKind::Cassandra {
        platypus_driver_cassandra::cql::split(script)
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
    mut connection: SavedConnection,
    password: PasswordInput,
    ssh_secrets: Option<SshSecrets>,
    projects: Option<Vec<Uuid>>,
) -> CommandResult<Vec<SavedConnection>> {
    platypus_store::tidy_address(&mut connection);
    state.with_store(|s| {
        if platypus_store::name_taken(s.list(), &connection) {
            return Err(platypus_store::StoreError::Invalid(format!(
                "a connection named \"{}\" exists already",
                connection.name.trim()
            )));
        }
        Ok(())
    })?;
    let id = connection.id;
    let update = match password {
        PasswordInput::Keep => PasswordUpdate::Keep,
        PasswordInput::Set(p) => PasswordUpdate::Set(Secret::new(p)),
        PasswordInput::Clear => PasswordUpdate::Clear,
    };
    // Statements the user typed here are theirs: they never ask. Those a shared file brought
    // and the form left as they were still wait for approval.
    let written = state.with_store(|s| {
        let before = s.get(id).map(|c| c.init.statements.as_str());
        Ok(before != Some(connection.init.statements.as_str())
            && !connection.init.statements.trim().is_empty())
    })?;
    if written {
        let hash = init_hash(&connection.init.statements);
        state.with_settings(|s| {
            s.set(
                &approval_key(id),
                Some(platypus_store::toml::Value::String(hash)),
            )
        })?;
    }
    let (access, values, limits) = (
        connection.agent_access,
        connection.agent_values,
        connection.agent_limits,
    );
    let before = state.with_store(|s| {
        Ok(s.get(id)
            .map(|c| (c.agent_access, c.agent_values, c.agent_limits)))
    })?;
    let saved = state.with_store(|s| {
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
    })?;
    // The agent access chosen here is the user's: agents get it (see `agent_policy`).
    state.approve_agent_access(id, before.map(|b| b.0), access);
    state.approve_agent_values(id, before.map(|b| b.1), values);
    state.approve_agent_limits(id, before.map(|b| b.2), limits);
    Ok(saved)
}

/// Connection files of other tools found where they keep them.
#[tauri::command]
pub fn detect_imports() -> Vec<ImportFile> {
    platypus_store::detect_imports()
}

/// The projects and connections of another tool's file, or of a PlatypusDB export (an
/// encrypted one needs `password`), for the user to pick from; nothing is saved. The
/// passwords found stay in the backend for `save_imported`.
#[tauri::command]
pub fn read_import(
    state: State<'_, AppState>,
    source: ImportSource,
    path: PathBuf,
    password: Option<String>,
) -> CommandResult<Vec<ImportedProject>> {
    let projects = if source == ImportSource::Platypus {
        let bytes = std::fs::read(&path).map_err(|e| CommandError::new("io", e.to_string()))?;
        platypus_sync::export::read(&bytes, password.as_deref())
            .map_err(|e| super::sync::error(&e))?
    } else {
        platypus_store::import_connections(source, &path)?
    };
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

/// Writes the connections `request` picks to a `.platypusconn` file at `path`; with a
/// password the file is encrypted and may carry the saved passwords.
#[tauri::command]
pub fn export_connections(
    state: State<'_, AppState>,
    request: platypus_sync::export::ExportRequest,
    path: PathBuf,
) -> CommandResult<()> {
    let bytes = match &*lock(&state.store) {
        Ok(store) => platypus_sync::export::export(store, &request),
        Err(e) => return Err(CommandError::new("store", e.clone())),
    }
    .map_err(|e| super::sync::error(&e))?;
    platypus_sync::file::write_atomic(&path, &bytes).map_err(|e| super::sync::error(&e))
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
    let conn = open_for_test(
        &state,
        &connection,
        password,
        ssh_secrets.unwrap_or_default(),
    )
    .await?;
    Ok(conn.server_info().clone())
}

/// The option names that point a driver at its server (a URL, a DSN, an account…).
const ADDRESS_OPTIONS: [&str; 12] = [
    "host",
    "url",
    "uri",
    "dsn",
    "connection",
    "account",
    "endpoint",
    "server",
    "proxy",
    "port",
    "domain",
    "region",
];

/// Whether the form `form` still reaches the server of the saved connection `saved`: the
/// same driver, host, port, tunnel (the SSH servers it goes through, network profile) and
/// options that name the server, and the same database where a database can name another
/// place. Only then may the password kept for `saved` go with it; a form pointed elsewhere is
/// tested with a typed password only. How the SSH server is signed in to moves nothing: its
/// own secrets are [`kept_ssh_secrets`].
fn same_target(form: &SavedConnection, saved: &SavedConnection) -> bool {
    let route = |c: &SavedConnection| -> Vec<(String, u16)> {
        c.ssh
            .iter()
            .flat_map(SavedSsh::chain)
            .map(|hop| (hop.host.trim().to_owned(), hop.port))
            .collect()
    };
    // Only a name on these servers; elsewhere it may be a descriptor or a client's alias.
    let names_only = matches!(form.driver, DriverKind::Postgres | DriverKind::MySql)
        && !form.host.trim().is_empty();
    let address = |c: &SavedConnection| -> Vec<(String, String)> {
        c.options
            .iter()
            .filter(|(k, _)| {
                let k = k.to_ascii_lowercase();
                // `connect_timeout` and the like change how, not where.
                !k.contains("timeout") && ADDRESS_OPTIONS.iter().any(|a| k.contains(a))
            })
            .map(|(k, v)| (k.clone(), v.trim().to_owned()))
            .collect()
    };
    form.driver == saved.driver
        && form.host.trim() == saved.host.trim()
        && form.port == saved.port
        && (names_only || form.database.trim() == saved.database.trim())
        && route(form) == route(saved)
        && form.network_profile == saved.network_profile
        && address(form) == address(saved)
}

/// The form's SSH secrets `typed`, where each server of the form's chain that would use its
/// kept password or passphrase does so only when the saved connection `saved` has it with the
/// same host, port, user and sign-in: like the database password ([`same_target`]), a kept
/// SSH secret never goes to a server the form changed to (nor to one a new connection's form
/// names by another connection's `secret_id`).
fn kept_ssh_secrets(
    form: &SavedConnection,
    saved: Option<&SavedConnection>,
    mut typed: SshSecrets,
) -> SshSecrets {
    let saved: Vec<&SavedSsh> = saved
        .and_then(|s| s.ssh.as_ref())
        .into_iter()
        .flat_map(SavedSsh::chain)
        .collect();
    for hop in form.ssh.iter().flat_map(SavedSsh::chain) {
        let keeps = matches!(typed.get(&hop.secret_id), None | Some(PasswordInput::Keep));
        let same = saved.iter().any(|s| {
            s.secret_id == hop.secret_id
                && s.host.trim() == hop.host.trim()
                && s.port == hop.port
                && s.user.trim() == hop.user.trim()
                && s.auth == hop.auth
                && s.key_path == hop.key_path
        });
        if keeps && !same {
            typed.insert(hop.secret_id, PasswordInput::Clear);
        }
    }
    typed
}

/// Opens the connection `connection` describes, with the password the form typed, else the
/// one it keeps when the form still reaches the saved server ([`same_target`]), and the SSH
/// secrets alike ([`kept_ssh_secrets`]); shared by the connection test and the driver
/// compatibility probe.
pub(crate) async fn open_for_test(
    state: &AppState,
    connection: &SavedConnection,
    password: PasswordInput,
    ssh: SshSecrets,
) -> CommandResult<Box<dyn Connection>> {
    // A connection that was never saved has nothing stored yet.
    let saved = state.with_store(|s| Ok(s.get(connection.id).cloned()))?;
    let keeps = matches!(password, PasswordInput::Keep);
    let secret = match password {
        PasswordInput::Set(p) => Some(Secret::new(p)),
        PasswordInput::Clear => None,
        PasswordInput::Keep => match external_password(connection).await? {
            Some(secret) => Some(secret),
            None => match &saved {
                Some(saved) if same_target(connection, saved) => {
                    state.with_store(|s| s.password(connection.id))?
                }
                _ => None,
            },
        },
    };
    let ssh = kept_ssh_secrets(connection, saved.as_ref(), ssh);
    // Asked for by name below, when the server wants the password that was held back. A keychain
    // that cannot answer (a locked vault) is not this test's failure: the server's answer stands.
    let withheld = keeps
        && secret.is_none()
        && saved.is_some()
        && state
            .with_store(|s| s.password(connection.id))
            .is_ok_and(|kept| kept.is_some());
    let config = state
        .with_store(|s| Ok(config_of(s, connection, secret, &ssh)))?
        .with_env()?;
    check_address(&config)?;
    connect(&config)
        .await
        .map_err(|e| said_withheld(e.into(), withheld))
}

/// A refused login, when the kept password stayed home because the form points elsewhere, as
/// the kind the form asks the password for: the driver can only say it is missing.
fn said_withheld(e: CommandError, withheld: bool) -> CommandError {
    if withheld && login_refused(&e) {
        e.with_kind("kept_password_withheld")
    } else {
        e
    }
}

/// What a connection needs to be tried at all, said at once rather than by the TLS or the
/// network layer: a host, and the host and user of each SSH server.
fn check_address(config: &ConnectionConfig) -> CommandResult<()> {
    if config.driver.is_embedded() {
        return Ok(());
    }
    if config.driver.needs_host() && config.host.trim().is_empty() {
        return Err(Error::Config("the host is missing".to_owned()).into());
    }
    config.check_host()?;
    let mut ssh = config.ssh.as_ref();
    while let Some(server) = ssh {
        if server.host.trim().is_empty() {
            return Err(Error::Config("the SSH host is missing".to_owned()).into());
        }
        if server.user.trim().is_empty() {
            return Err(Error::Config("the SSH user is missing".to_owned()).into());
        }
        ssh = server.jump.as_deref();
    }
    Ok(())
}

/// Whether a failed login is about the credentials, so that asking for the password again
/// may help. The drivers word it their own way: libSQL answers "Unauthorized", a plugin
/// may pass on its server's HTTP 401.
fn login_refused(e: &CommandError) -> bool {
    let text = e.message().to_ascii_lowercase();
    let status_401 = text.match_indices("401").any(|(at, _)| {
        let digit = |c: Option<char>| c.is_some_and(|c| c.is_ascii_digit());
        !digit(text[..at].chars().next_back()) && !digit(text[at + 3..].chars().next())
    });
    status_401
        || [
            "password",
            "authenticat",
            "access denied",
            "login failed",
            "credentials",
            "unauthorized",
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
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    connection_id: Uuid,
    password: Option<String>,
    database: Option<String>,
    side: Option<bool>,
    init: Option<InitChoice>,
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
    } = saved_config(
        &state,
        connection_id,
        password,
        database,
        init.unwrap_or(InitChoice::Ask),
    )
    .await?;
    let side = side == Some(true);
    let production = production && !side;
    // `auth`: the UI asks for the password (again), with the server's answer.
    let info = start_session(&state, connection_id, config, label, (production, side))
        .await
        .map_err(|e| {
            if (typed || none_saved) && login_refused(&e) {
                e.with_kind("auth")
            } else {
                e
            }
        })?;
    // A dialog's own session is no new connection to the user.
    if !side && HOOKS.wanted(HookEvent::OnConnect) {
        super::plugins::on_connect(&app, &state, info.session_id);
    }
    Ok(info)
}

/// What to do with the init statements a shared project's file brought, which this machine's
/// user has not approved yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitChoice {
    /// Refuse to connect with an `init_approval` error holding the statements, so the UI
    /// shows them.
    Ask,
    /// The user read them: run them, and remember that exact text.
    Run,
    /// Connect without them.
    Skip,
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

/// Applies the user's say over the init statements of a connection in a shared project: text
/// this machine has not approved is dropped from `saved`, unless `init` is `Run` (which
/// remembers the approval) or asks (`Ask`, an error carrying the text).
fn check_init(
    state: &AppState,
    saved: &mut SavedConnection,
    init: InitChoice,
) -> CommandResult<()> {
    let shared = state.with_store(|s| {
        Ok(s.projects()
            .iter()
            .any(|p| p.shared.is_some() && p.connections.contains(&saved.id)))
    })?;
    let key = approval_key(saved.id);
    let approved: Option<String> = state.setting(&key);
    if !needs_approval(shared, &saved.init.statements, approved.as_deref()) {
        return Ok(());
    }
    match init {
        InitChoice::Ask => {
            return Err(CommandError::new(
                "init_approval",
                saved.init.statements.clone(),
            ));
        }
        InitChoice::Run => {
            let hash = init_hash(&saved.init.statements);
            state
                .with_settings(|s| s.set(&key, Some(platypus_store::toml::Value::String(hash))))?;
        }
        InitChoice::Skip => saved.init.statements.clear(),
    }
    Ok(())
}

/// The settings of saved connection `connection_id`, on `database` when given, with
/// `password`, or the one its password command prints, or the saved one; environment
/// variables replaced.
pub(crate) async fn saved_config(
    state: &AppState,
    connection_id: Uuid,
    password: Option<String>,
    database: Option<String>,
    init: InitChoice,
) -> CommandResult<SavedConfig> {
    let mut saved = state.with_store(|s| {
        s.get(connection_id)
            .cloned()
            .ok_or(platypus_store::StoreError::NotFound(connection_id))
    })?;
    check_init(state, &mut saved, init)?;
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
        && !saved.pgpass
        && !saved.signs_in_without_password();
    let mut config = state
        .with_store(|s| Ok(config_of(s, &saved, secret, &SshSecrets::new())))?
        .with_env()?;
    check_address(&config)?;
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

#[cfg(test)]
mod tests {
    use platypus_store::{MemorySecrets, Project, SettingsStore, SharedLink};

    use crate::commands::analysis::error_kind;

    use super::*;

    /// The kept password goes only to the server it was kept for: a form pointed at another
    /// host, port, driver, tunnel or URL option is tested without it.
    #[test]
    fn the_kept_password_goes_only_to_its_own_server() {
        let mut saved = SavedConnection::new("Shop", DriverKind::Postgres);
        saved.host = "db.internal".to_owned();
        saved.port = Some(5432);
        saved.database = "shop".to_owned();
        saved
            .options
            .insert("connect_timeout".to_owned(), "10".to_owned());
        let mut form = saved.clone();
        form.name = "Shop renamed".to_owned();
        form.user = "other".to_owned();
        form.read_only = true;
        form.options
            .insert("connect_timeout".to_owned(), "30".to_owned());
        assert!(
            same_target(&form, &saved),
            "only the name and settings changed"
        );
        form.host = " db.internal ".to_owned();
        assert!(same_target(&form, &saved));
        form.database = "another of the same server".to_owned();
        assert!(same_target(&form, &saved), "a database is only a name here");

        // Elsewhere a database can say where the server is.
        let mut oracle = saved.clone();
        oracle.driver = DriverKind::Oracle;
        let mut descriptor = oracle.clone();
        descriptor.database = "(DESCRIPTION=(ADDRESS=(HOST=attacker.example)))".to_owned();
        assert!(!same_target(&descriptor, &oracle));

        let changes: [fn(&mut SavedConnection); 6] = [
            |c| c.host = "attacker.example".to_owned(),
            |c| c.port = Some(6543),
            |c| c.driver = DriverKind::MySql,
            |c| c.network_profile = Some(Uuid::new_v4()),
            |c| {
                c.ssh = Some(SavedSsh {
                    host: "jump.example".to_owned(),
                    port: 22,
                    user: "me".to_owned(),
                    auth: platypus_store::SshAuthKind::Agent,
                    key_path: None,
                    secret_id: Uuid::nil(),
                    jump: None,
                });
            },
            |c| {
                c.options
                    .insert("url".to_owned(), "postgres://attacker.example/x".to_owned());
            },
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut form = saved.clone();
            change(&mut form);
            assert!(!same_target(&form, &saved), "change {i}");
        }
    }

    /// The tunnel moves the password only when it goes through other servers: another way of
    /// signing in to the same SSH server still ends at the same database.
    #[test]
    fn signing_in_to_the_same_ssh_server_otherwise_keeps_the_password() {
        let tunnel = |host: &str, user: &str, key: Option<&str>| SavedSsh {
            host: host.to_owned(),
            port: 22,
            user: user.to_owned(),
            auth: platypus_store::SshAuthKind::Agent,
            key_path: key.map(PathBuf::from),
            secret_id: Uuid::nil(),
            jump: None,
        };
        let mut saved = SavedConnection::new("Shop", DriverKind::Postgres);
        saved.host = "localhost".to_owned();
        saved.ssh = Some(tunnel("jump.internal", "me", None));
        let mut form = saved.clone();
        form.ssh = Some(tunnel("jump.internal", "other", Some("id_rsa")));
        assert!(same_target(&form, &saved));
        form.ssh = Some(tunnel("jump.example", "me", None));
        assert!(!same_target(&form, &saved), "localhost of another server");
        form.ssh = None;
        assert!(!same_target(&form, &saved), "this computer's localhost");
    }

    /// The form asks for the password only when it was held back and the server wanted one.
    #[test]
    fn only_a_refused_login_says_the_password_was_withheld() {
        let refused = || CommandError::new("database", "password missing");
        assert_eq!(
            said_withheld(refused(), true).kind(),
            "kept_password_withheld"
        );
        assert_eq!(said_withheld(refused(), false).kind(), "database");
        let down = CommandError::new("database", "connection refused");
        assert_eq!(said_withheld(down, true).kind(), "database");
    }

    /// A kept SSH password or passphrase goes only to the SSH server it was kept for.
    #[test]
    fn kept_ssh_secrets_go_only_to_their_own_server() {
        let jump = SavedSsh {
            host: "jump.internal".to_owned(),
            port: 22,
            user: "me".to_owned(),
            auth: platypus_store::SshAuthKind::Password,
            key_path: None,
            secret_id: Uuid::new_v4(),
            jump: None,
        };
        let bastion = SavedSsh {
            host: "bastion.internal".to_owned(),
            secret_id: Uuid::new_v4(),
            jump: Some(Box::new(jump.clone())),
            ..jump.clone()
        };
        let mut saved = SavedConnection::new("Shop", DriverKind::Postgres);
        saved.ssh = Some(bastion.clone());
        let cleared = |form: &SavedConnection, typed: SshSecrets| -> Vec<Uuid> {
            let mut ids: Vec<Uuid> = kept_ssh_secrets(form, Some(&saved), typed)
                .into_iter()
                .filter(|(_, input)| matches!(input, PasswordInput::Clear))
                .map(|(id, _)| id)
                .collect();
            ids.sort();
            ids
        };
        assert_eq!(cleared(&saved, SshSecrets::new()), Vec::<Uuid>::new());

        // The bastion moved to another host: its kept password stays home, the jump's goes.
        let mut form = saved.clone();
        form.ssh.as_mut().unwrap().host = "attacker.example".to_owned();
        assert_eq!(cleared(&form, SshSecrets::new()), [bastion.secret_id]);
        // A password typed for it is the user's own choice.
        let typed = SshSecrets::from([(bastion.secret_id, PasswordInput::Set("x".to_owned()))]);
        assert_eq!(cleared(&form, typed), Vec::<Uuid>::new());

        let changes: [fn(&mut SavedSsh); 4] = [
            |s| s.port = 2222,
            |s| s.user = "root".to_owned(),
            |s| s.auth = platypus_store::SshAuthKind::Key,
            |s| s.key_path = Some("/tmp/other".into()),
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut form = saved.clone();
            change(form.ssh.as_mut().unwrap().jump.as_mut().unwrap());
            assert_eq!(
                cleared(&form, SshSecrets::new()),
                [jump.secret_id],
                "change {i}"
            );
        }

        // A form that was never saved keeps no SSH secret, whatever ids it names.
        let mut ids = vec![bastion.secret_id, jump.secret_id];
        ids.sort();
        let fresh = kept_ssh_secrets(&saved, None, SshSecrets::new());
        let mut cleared_fresh: Vec<Uuid> = fresh.into_keys().collect();
        cleared_fresh.sort();
        assert_eq!(cleared_fresh, ids);
    }

    #[test]
    fn a_connection_without_a_host_or_an_ssh_host_is_refused_at_once() {
        let mut config = ConnectionConfig::new(DriverKind::Postgres);
        config.host.clear();
        assert!(
            check_address(&config)
                .unwrap_err()
                .message()
                .contains("host is missing")
        );
        config.host = "db.internal".to_owned();
        assert!(check_address(&config).is_ok());
        let ssh = |host: &str, user: &str| platypus_core::SshConfig {
            host: host.to_owned(),
            port: 22,
            user: user.to_owned(),
            auth: platypus_core::SshAuth::Agent,
            jump: None,
        };
        config.ssh = Some(ssh("", "ops"));
        assert!(
            check_address(&config)
                .unwrap_err()
                .message()
                .contains("SSH host")
        );
        config.ssh = Some(ssh("bastion", ""));
        assert!(
            check_address(&config)
                .unwrap_err()
                .message()
                .contains("SSH user")
        );
        config.ssh = Some(ssh("bastion", "ops"));
        assert!(check_address(&config).is_ok());
        // A server reached by a URL or an account has no host field.
        assert!(check_address(&ConnectionConfig::new(DriverKind::Snowflake)).is_ok());
        // An ODBC data source or connection string holds the server, an Oracle net service
        // name or connect descriptor too, and a plugin may know its endpoint.
        for driver in [DriverKind::Odbc, DriverKind::Oracle, DriverKind::Plugin] {
            let mut config = ConnectionConfig::new(driver);
            config.host.clear();
            assert!(check_address(&config).is_ok(), "{driver:?}");
        }
        // ClickHouse is not assumed on this machine.
        let mut config = ConnectionConfig::new(DriverKind::ClickHouse);
        config.host = " ".to_owned();
        assert!(check_address(&config).is_err());
    }

    #[test]
    fn a_refused_login_is_told_by_the_words_of_each_driver() {
        let refused = |message: &str| {
            let e: CommandError = Error::Config(message.to_owned()).into();
            login_refused(&e)
        };
        for message in [
            "password authentication failed for user \"app\"",
            "Access denied for user 'app'@'%'",
            "Login failed for user 'sa'.",
            "server returned 401 Unauthorized",
            "HTTP status 401",
            "Unauthorized",
            "invalid credentials",
        ] {
            assert!(refused(message), "{message}");
        }
        for message in [
            "connection refused",
            "could not connect to 10.0.0.1:4010",
            "error at line 1401",
            "HTTP status 503",
        ] {
            assert!(!refused(message), "{message}");
        }
    }

    /// The connection form refuses an empty host for the drivers the backend does.
    #[test]
    fn the_form_and_the_backend_agree_on_the_drivers_without_a_host() {
        let source = include_str!("../../../src/connectionUrl.ts");
        let list = source
            .split_once("HOST_OPTIONAL: readonly DriverKind[] = [")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(list, _)| list)
            .expect("HOST_OPTIONAL in connectionUrl.ts");
        let form: Vec<DriverKind> = list
            .split(',')
            .map(|name| serde_json::from_str(name.trim()).unwrap())
            .collect();
        let all = [
            DriverKind::Postgres,
            DriverKind::MySql,
            DriverKind::SqlServer,
            DriverKind::ClickHouse,
            DriverKind::Libsql,
            DriverKind::Mongo,
            DriverKind::Redis,
            DriverKind::Elasticsearch,
            DriverKind::Cassandra,
            DriverKind::Snowflake,
            DriverKind::BigQuery,
            DriverKind::Databricks,
            DriverKind::Trino,
            DriverKind::Adbc,
            DriverKind::Oracle,
            DriverKind::Db2,
            DriverKind::Odbc,
            DriverKind::Plugin,
        ];
        for driver in all {
            assert_eq!(
                form.contains(&driver),
                !driver.needs_host(),
                "{driver:?} in HOST_OPTIONAL"
            );
        }
    }

    fn none<T>() -> Result<T, String> {
        Err("not opened in tests".to_owned())
    }

    /// A state with one connection holding `statements`, in a shared project or not.
    fn state_with(
        statements: &str,
        shared: bool,
    ) -> (AppState, SavedConnection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ConnectionStore::open(
            dir.path().join("connections.toml"),
            Arc::new(MemorySecrets::default()),
        )
        .unwrap();
        let mut saved = SavedConnection::new("Ledger", DriverKind::Sqlite);
        saved.init.statements = statements.to_owned();
        store.save(saved.clone(), PasswordUpdate::Keep).unwrap();
        let mut project = Project::new("Team", "#112233");
        project.connections = vec![saved.id];
        project.shared = shared.then(|| SharedLink {
            path: dir.path().join("shared"),
            share_user: false,
        });
        store.save_project(project).unwrap();
        let state = AppState::new(
            (Ok(store), None),
            none(),
            none(),
            none(),
            none(),
            Ok(SettingsStore::open(dir.path().join("settings.toml")).unwrap()),
            none(),
        );
        (state, saved, dir)
    }

    #[test]
    fn shared_init_statements_ask_until_approved_and_ask_again_when_changed() {
        let (state, saved, _dir) = state_with("SET x = 1;", true);

        let err = check_init(&state, &mut saved.clone(), InitChoice::Ask).unwrap_err();
        assert_eq!(
            (error_kind(&err).as_str(), err.message()),
            ("init_approval", "SET x = 1;")
        );

        let mut skipped = saved.clone();
        check_init(&state, &mut skipped, InitChoice::Skip).unwrap();
        assert_eq!(skipped.init.statements, "");
        // Skipping remembers nothing.
        assert!(check_init(&state, &mut saved.clone(), InitChoice::Ask).is_err());

        let mut run = saved.clone();
        check_init(&state, &mut run, InitChoice::Run).unwrap();
        assert_eq!(run.init.statements, "SET x = 1;");
        let mut again = saved.clone();
        check_init(&state, &mut again, InitChoice::Ask).unwrap();
        assert_eq!(again.init.statements, "SET x = 1;");

        let mut changed = saved;
        changed.init.statements = "SET x = 2;".to_owned();
        assert!(check_init(&state, &mut changed, InitChoice::Ask).is_err());
    }

    #[test]
    fn statements_outside_a_shared_project_never_ask() {
        let (state, saved, _dir) = state_with("SET x = 1;", false);
        let mut c = saved;
        check_init(&state, &mut c, InitChoice::Ask).unwrap();
        assert_eq!(c.init.statements, "SET x = 1;");
    }
}
