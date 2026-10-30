//! The commands of the saved results: a result's rows kept as a Parquet file (see
//! `crate::saved_results`) and opened later without their connection, as the table `result`
//! of an in-memory DuckDB that a query tab reads.

use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::AppState;
use super::analysis::{gone, sql};
use super::summary::blocking;
use crate::error::{CommandError, CommandResult};
use crate::saved_results::{Origin, SavedResult, SavedResults};

/// A store error, for the page to show.
fn failed(message: String) -> CommandError {
    CommandError::new("saved_result", message)
}

/// A saved result read back, with the temporary connection to its rows.
#[derive(Debug, Serialize)]
pub struct OpenedSavedResult {
    connection: platypus_store::SavedConnection,
    result: SavedResult,
}

/// Saves the rows result `job_id` has loaded; `partial` in the answer when they are not all
/// of its rows. A connection that keeps its data in refuses, as an export does.
#[tauri::command]
pub async fn save_result(
    state: State<'_, AppState>,
    saved: State<'_, SavedResults>,
    job_id: u64,
    origin: Origin,
) -> CommandResult<SavedResult> {
    let job = state.job(job_id).map_err(|_| gone())?;
    state.exportable(job.session_id)?;
    let (schema, batches) = job.snapshot().ok_or_else(gone)?;
    let partial = !job.is_complete();
    let store = (*saved).clone();
    blocking(move || {
        store
            .add(origin, &schema, &batches, partial)
            .map_err(failed)
    })
    .await
}

/// The saved results, the latest first.
#[tauri::command]
pub fn list_saved_results(saved: State<'_, SavedResults>) -> Vec<SavedResult> {
    saved.list()
}

/// Renames a saved result.
#[tauri::command]
pub fn rename_saved_result(
    saved: State<'_, SavedResults>,
    id: Uuid,
    name: String,
) -> CommandResult<SavedResult> {
    saved.rename(id, &name).map_err(failed)
}

/// Deletes a saved result and its file.
#[tauri::command]
pub fn delete_saved_result(saved: State<'_, SavedResults>, id: Uuid) -> CommandResult<()> {
    saved.delete(id).map_err(failed)
}

/// Reads a saved result into a new in-memory DuckDB, as the table `result`, and returns the
/// temporary connection to it, the way "Query with SQL" does for a result on screen. No
/// connection to the original database is needed.
#[tauri::command]
pub async fn open_saved_result(
    state: State<'_, AppState>,
    saved: State<'_, SavedResults>,
    id: Uuid,
    name: String,
) -> CommandResult<OpenedSavedResult> {
    let result = saved.get(id).map_err(failed)?;
    let store = (*saved).clone();
    let (schema, batches) = blocking(move || store.read(id).map_err(failed)).await?;
    let connection = sql::open_rows(&state, &name, schema, batches).await?;
    Ok(OpenedSavedResult { connection, result })
}
