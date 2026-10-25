//! Saved connections and the sessions opened from them.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use platypus_core::{Capabilities, Connection, ConnectionConfig, DriverKind, Secret, ServerInfo};
use platypus_sql::Dialect;
use platypus_store::{
    ConnectionStore, ConnectionType, ImportFile, ImportSource, NetworkProfile, PasswordUpdate,
    SavedConnection, SavedSsh,
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

/// The connections of another tool's file, for the user to pick from; nothing is saved.
#[tauri::command]
pub fn read_import(source: ImportSource, path: PathBuf) -> CommandResult<Vec<SavedConnection>> {
    Ok(platypus_store::import_connections(source, &path)?)
}

/// Saves imported connections, into `project` when one is given; they ask for their
/// passwords on first use.
#[tauri::command]
pub fn save_imported(
    state: State<'_, AppState>,
    connections: Vec<SavedConnection>,
    project: Option<Uuid>,
) -> CommandResult<Vec<SavedConnection>> {
    state.with_store(|s| {
        // A project deleted meanwhile is refused before anything is saved.
        if let Some(project) = project
            && !s.projects().iter().any(|p| p.id == project)
        {
            return Err(platypus_store::StoreError::NotFound(project));
        }
        let ids: Vec<Uuid> = connections.iter().map(|c| c.id).collect();
        for connection in connections {
            s.save(connection, PasswordUpdate::Clear)?;
        }
        if let Some(project) = project {
            s.add_to_project(project, &ids)?;
        }
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
    let config = state.with_store(|s| Ok(config_of(s, &connection, secret, &ssh)))?;
    let conn = connect(&config).await?;
    Ok(conn.server_info().clone())
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    session_id: u64,
    server: ServerInfo,
    /// What the driver supports, so the UI can hide actions it would reject.
    capabilities: Capabilities,
}

/// Opens a session for a saved connection. `password` overrides the stored one, `database`
/// the saved database (another database of the same server).
#[tauri::command]
pub async fn open_session(
    state: State<'_, AppState>,
    connection_id: Uuid,
    password: Option<String>,
    database: Option<String>,
) -> CommandResult<SessionInfo> {
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
    let production = saved.connection_type == Some(ConnectionType::Prod);
    let mut config = state.with_store(|s| Ok(config_of(s, &saved, secret, &SshSecrets::new())))?;
    let label = match &database {
        Some(database) => format!("{} · {database}", saved.name),
        None => saved.name.clone(),
    };
    if let Some(database) = database {
        config.database = database;
    }
    let raw: Arc<dyn Connection> = Arc::from(connect(&config).await?);
    let conn = Logged::wrap(Arc::clone(&raw), &state.query_log, &label);
    // Production starts in manual commit, so nothing changes until Commit.
    if production && conn.capabilities().transactions {
        conn.set_autocommit(false).await?;
    }
    let server = conn.server_info().clone();
    let capabilities = conn.capabilities();
    let session_id = state.next_id();
    lock(&state.sessions).insert(
        session_id,
        Session {
            connection_id,
            conn,
            config,
            label,
            raw,
        },
    );
    Ok(SessionInfo {
        session_id,
        server,
        capabilities,
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
}
