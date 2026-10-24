//! The query manager: what the app sent to the databases.

use tauri::State;

use super::AppState;
use crate::querylog::LogPage;

/// The log entries that changed after revision `since` (0: all of them).
#[tauri::command]
pub fn query_log(state: State<'_, AppState>, since: u64) -> LogPage {
    state.query_log.page(since)
}

#[tauri::command]
pub fn clear_query_log(state: State<'_, AppState>) {
    state.query_log.clear();
}
