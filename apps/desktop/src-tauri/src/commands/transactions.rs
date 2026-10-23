//! Autocommit and explicit transactions of an editor session.

use platypus_core::TransactionState;
use tauri::State;

use super::AppState;
use super::query::stop_jobs;
use crate::error::CommandResult;

#[tauri::command]
pub fn transaction_state(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<TransactionState> {
    Ok(state.session(session_id)?.conn.transaction())
}

#[tauri::command]
pub async fn set_autocommit(
    state: State<'_, AppState>,
    session_id: u64,
    on: bool,
) -> CommandResult<TransactionState> {
    let conn = state.live(session_id).await?;
    stop_jobs(&state, session_id);
    conn.set_autocommit(on).await?;
    Ok(conn.transaction())
}

/// Commits (`commit: true`) or rolls back the session's transaction. Results still being
/// fetched are stopped first: they hold the session.
#[tauri::command]
pub async fn end_transaction(
    state: State<'_, AppState>,
    session_id: u64,
    commit: bool,
) -> CommandResult<TransactionState> {
    let conn = state.live(session_id).await?;
    stop_jobs(&state, session_id);
    if commit {
        conn.commit().await?;
    } else {
        conn.rollback().await?;
    }
    Ok(conn.transaction())
}
