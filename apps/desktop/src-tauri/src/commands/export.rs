//! Exports: a query re-run in streaming, or the rows a result has loaded.

use std::path::PathBuf;

use platypus_export::ExportOptions;
use tauri::State;
use tauri::ipc::Channel;

use super::{AppState, lock};
use crate::error::{CommandError, CommandResult};
use crate::export::{Export, ExportEvent};

/// Re-runs `sql` on a new connection and streams all its rows to `path`. Returns an id for
/// `cancel_export`; progress and the outcome arrive on `on_event`.
#[tauri::command]
pub fn export_query(
    state: State<'_, AppState>,
    session_id: u64,
    sql: String,
    path: PathBuf,
    options: ExportOptions,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    let config = state.session(session_id)?.config;
    let export = Export::start(config, sql, path, options, on_event);
    let id = state.next_id();
    lock(&state.exports).insert(id, export);
    Ok(id)
}

#[tauri::command]
pub fn cancel_export(state: State<'_, AppState>, export_id: u64) {
    if let Some(export) = lock(&state.exports).remove(&export_id) {
        export.cancel();
    }
}

/// Writes the rows a result has loaded so far; returns how many.
#[tauri::command]
pub async fn export_rows(
    state: State<'_, AppState>,
    job_id: u64,
    path: PathBuf,
    options: ExportOptions,
) -> CommandResult<u64> {
    let (schema, batches) = state
        .job(job_id)?
        .snapshot()
        .ok_or_else(|| CommandError::new("not_ready", "the result has no rows"))?;
    tokio::task::spawn_blocking(move || {
        crate::export::write_batches(&path, &schema, &batches, &options)
    })
    .await
    .map_err(|e| CommandError::new("export", e.to_string()))?
    .map_err(Into::into)
}
