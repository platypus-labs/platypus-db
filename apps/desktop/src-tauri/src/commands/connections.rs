//! Saved connections and the sessions opened from them.

use std::sync::Arc;

use platypus_core::{Capabilities, Connection, DriverKind, Secret, ServerInfo};
use platypus_store::{PasswordUpdate, SavedConnection};
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
    let mut config = config;
    if let Some(database) = database {
        config.database = database;
    }
    let conn: Arc<dyn Connection> = Arc::from(connect(&config).await?);
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
