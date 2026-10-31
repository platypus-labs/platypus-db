//! The server's sessions and locks, its costliest statements and the use of its tables, for
//! the admin tab; and the upkeep of a table.

use platypus_core::{
    EndSession, Maintenance, ObjectStatistics, ServerLock, ServerMetrics, ServerParameter,
    ServerQuery, ServerSession,
};
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

#[tauri::command]
pub async fn top_queries(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<Vec<ServerQuery>> {
    let conn = state.live(session_id).await?;
    Ok(conn.top_queries().await?)
}

#[tauri::command]
pub async fn object_statistics(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<Vec<ObjectStatistics>> {
    let conn = state.live(session_id).await?;
    Ok(conn.object_statistics().await?)
}

/// Runs `operation` on table `schema.table` and returns what the server said of it; not on
/// a read-only connection.
#[tauri::command]
pub async fn maintain_table(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    table: String,
    operation: Maintenance,
) -> CommandResult<Vec<String>> {
    maintain(&state, session_id, &schema, &table, operation).await
}

/// [`maintain_table`]. A read-only connection refuses it before anything reaches the server,
/// whichever of its sessions asks: a tab's, or the navigator's side session the Maintenance
/// dialog uses (each knows its saved connection, whose setting is read at each call).
pub(crate) async fn maintain(
    state: &AppState,
    session_id: u64,
    schema: &str,
    table: &str,
    operation: Maintenance,
) -> CommandResult<Vec<String>> {
    state.writable(session_id)?;
    let conn = state.live(session_id).await?;
    // Outside a transaction: VACUUM refuses one, and the others would keep their locks (or
    // commit what was pending, in MySQL) on a session in manual-commit mode.
    super::one_off(&conn, async {
        Ok(conn.maintain(schema, table, operation).await?)
    })
    .await
}

/// The server's configuration parameters, with their defaults where it tells them.
#[tauri::command]
pub async fn server_parameters(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<Vec<ServerParameter>> {
    let conn = state.live(session_id).await?;
    Ok(conn.server_parameters().await?)
}

/// A snapshot of the server's load, for the dashboard.
#[tauri::command]
pub async fn server_metrics(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<ServerMetrics> {
    let conn = state.live(session_id).await?;
    Ok(conn.server_metrics().await?)
}
