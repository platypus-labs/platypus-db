//! The data of a result's chart, computed from the rows it has loaded.

use platypus_local::chart::{self, ChartData, ChartRequest};
use tauri::State;

use super::loaded;
use crate::commands::AppState;
use crate::commands::summary::blocking;
use crate::display;
use crate::error::CommandResult;

/// What `request` draws from the rows result `job_id` has loaded.
#[tauri::command]
pub async fn chart_data(
    state: State<'_, AppState>,
    job_id: u64,
    request: ChartRequest,
) -> CommandResult<ChartData> {
    let rows = loaded(&state, job_id)?;
    blocking(move || {
        let options = display::format_options();
        Ok(chart::chart(
            &rows.schema,
            &rows.batches,
            &request,
            &options,
        )?)
    })
    .await
}
