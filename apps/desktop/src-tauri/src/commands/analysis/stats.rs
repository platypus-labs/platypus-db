//! The profile of a result column, shown under its header.

use platypus_local::stats::{self, ColumnProfile};
use serde::Serialize;
use tauri::State;

use super::loaded;
use crate::commands::AppState;
use crate::commands::summary::blocking;
use crate::display;
use crate::error::CommandResult;

#[derive(Debug, Serialize)]
pub struct ColumnProfileReply {
    #[serde(flatten)]
    profile: ColumnProfile,
    /// Computed over every row of the result, not only the loaded ones.
    complete: bool,
}

/// The profile of `column` over the rows result `job_id` has loaded.
#[tauri::command]
pub async fn column_profile(
    state: State<'_, AppState>,
    job_id: u64,
    column: usize,
) -> CommandResult<ColumnProfileReply> {
    let rows = loaded(&state, job_id)?;
    let complete = rows.complete;
    let profile = blocking(move || {
        let options = display::format_options();
        Ok(stats::profile(
            &rows.schema,
            &rows.batches,
            column,
            &options,
        )?)
    })
    .await?;
    Ok(ColumnProfileReply { profile, complete })
}
