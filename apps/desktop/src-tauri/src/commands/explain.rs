//! Query plans for the explain tab.

use platypus_core::Explain;
use tauri::State;

use super::AppState;
use crate::error::CommandResult;

/// The plan of one statement; with `analyze` it runs, and its changes are rolled back.
#[tauri::command]
pub async fn explain(
    state: State<'_, AppState>,
    session_id: u64,
    sql: String,
    analyze: bool,
) -> CommandResult<Explain> {
    let conn = state.live(session_id).await?;
    // `EXPLAIN SELECT 1;` is a valid request, but not with a semicolon then more text.
    let sql = sql.trim().trim_end_matches(';');
    Ok(conn.explain(sql, analyze).await?)
}
