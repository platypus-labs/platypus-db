//! Saved connections and the sessions opened from them.

use std::sync::Arc;

use std::collections::HashMap;
use std::path::PathBuf;

use platypus_core::{Capabilities, Connection, ConnectionConfig, DriverKind, Secret, ServerInfo};
use platypus_store::{
    ConnectionStore, ConnectionType, ImportFile, ImportSource, PasswordUpdate, SavedConnection,
    SavedSsh,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use super::{AppState, Session, connect, lock};
use crate::error::CommandResult;

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

fn config_of(
    store: &ConnectionStore,
    saved: &SavedConnection,
    password: Option<Secret>,
    ssh: &SshSecrets,
) -> ConnectionConfig {
    let mut config = saved.config(password);
    config.ssh = saved.ssh.as_ref().map(|s| ssh_config(store, s, ssh));
    config
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
        // Servers that left the chain take their secrets with them.
        let kept: Vec<Uuid> = connection
            .ssh
            .iter()
            .flat_map(SavedSsh::chain)
            .map(|h| h.secret_id)
            .collect();
        let dropped: Vec<Uuid> = s
            .get(connection.id)
            .and_then(|c| c.ssh.as_ref())
            .into_iter()
            .flat_map(SavedSsh::chain)
            .map(|h| h.secret_id)
            .filter(|id| !kept.contains(id))
            .collect();
        s.save(connection, update)?;
        for id in dropped {
            s.set_ssh_secret(id, None)?;
        }
        for (id, input) in ssh_secrets.unwrap_or_default() {
            match input {
                PasswordInput::Set(p) => s.set_ssh_secret(id, Some(&Secret::new(p)))?,
                PasswordInput::Clear => s.set_ssh_secret(id, None)?,
                PasswordInput::Keep => {}
            }
        }
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
    let config = state.with_store(|s| {
        let secret = match password {
            PasswordInput::Set(p) => Some(Secret::new(p)),
            PasswordInput::Clear => None,
            // A connection that was never saved has nothing stored yet.
            PasswordInput::Keep => match s.get(connection.id) {
                Some(_) => s.password(connection.id)?,
                None => None,
            },
        };
        Ok(config_of(s, &connection, secret, &ssh))
    })?;
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
    let (config, production) = state.with_store(|s| {
        let saved = s
            .get(connection_id)
            .ok_or(platypus_store::StoreError::NotFound(connection_id))?
            .clone();
        let secret = match password {
            Some(p) => Some(Secret::new(p)),
            None => s.password(connection_id)?,
        };
        let production = saved.connection_type == Some(ConnectionType::Prod);
        Ok((config_of(s, &saved, secret, &SshSecrets::new()), production))
    })?;
    let mut config = config;
    if let Some(database) = database {
        config.database = database;
    }
    let conn: Arc<dyn Connection> = Arc::from(connect(&config).await?);
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
        },
    );
    Ok(SessionInfo {
        session_id,
        server,
        capabilities,
    })
}

#[tauri::command]
pub fn close_session(state: State<'_, AppState>, session_id: u64) {
    lock(&state.sessions).remove(&session_id);
}
