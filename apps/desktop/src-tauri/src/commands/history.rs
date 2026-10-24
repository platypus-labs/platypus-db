//! Query history.

use platypus_store::{HistoryEntry, HistoryOrigin, HistoryQuery};
use tauri::State;
use uuid::Uuid;

use super::AppState;
use crate::error::CommandResult;

#[tauri::command]
pub fn search_history(
    state: State<'_, AppState>,
    connection_ids: Option<Vec<Uuid>>,
    text: Option<String>,
    before_id: Option<i64>,
    origin: Option<HistoryOrigin>,
    limit: usize,
) -> CommandResult<Vec<HistoryEntry>> {
    state.with_history(|h| {
        h.search(&HistoryQuery {
            connection_ids,
            text,
            before_id,
            origin,
            limit: limit.min(1000),
        })
    })
}

#[tauri::command]
pub fn delete_history(state: State<'_, AppState>, id: i64) -> CommandResult<()> {
    state.with_history(|h| h.delete(id))
}
