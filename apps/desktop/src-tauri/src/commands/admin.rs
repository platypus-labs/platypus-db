//! The server's sessions and locks for the admin tab.

use platypus_core::{EndSession, ServerLock, ServerSession};
use tauri::State;

use super::AppState;
use crate::error::CommandResult;

#[tauri::command]
pub async fn server_sessions(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<Vec<ServerSession>> {
    let conn = state.live(session_id).await?;
    Ok(conn.server_sessions().await?)
}

#[tauri::command]
pub async fn server_locks(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<Vec<ServerLock>> {
    let conn = state.live(session_id).await?;
    Ok(conn.server_locks().await?)
}

/// Cancels the statement of session `id` of the server, or ends the session; not on a
/// read-only connection.
#[tauri::command]
pub async fn end_server_session(
    state: State<'_, AppState>,
    session_id: u64,
    id: String,
    how: EndSession,
) -> CommandResult<()> {
    state.writable(session_id)?;
    let conn = state.live(session_id).await?;
    Ok(conn.end_server_session(&id, how).await?)
}
