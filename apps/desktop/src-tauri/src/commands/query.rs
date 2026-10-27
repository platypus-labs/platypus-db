//! Running statements and paging through their results.

use std::sync::Arc;
use std::time::SystemTime;

use platypus_core::Error;
use platypus_store::{HistoryStatus, NewHistoryEntry};
use tauri::State;
use tauri::ipc::{Channel, Response};

use super::{AppState, lock};
use crate::display;
use crate::error::{CommandError, CommandResult};
use crate::formats::DisplayFormat;
use crate::jobs::{Job, Outcome, QueryEvent};

/// Starts a statement and returns its job id at once; progress arrives on `on_event`.
/// `limit` caps the rows fetched and frees the session afterwards (script mode). `history`
/// false keeps the run out of the query history, e.g. an auto-refresh.
#[tauri::command]
pub async fn run_query(
    state: State<'_, AppState>,
    session_id: u64,
    sql: String,
    limit: Option<usize>,
    history: Option<bool>,
    on_event: Channel<QueryEvent>,
) -> CommandResult<u64> {
    let keep = history.unwrap_or(true);
    state.may_run(session_id, &sql)?;
    let conn = state.live(session_id).await?;
    let session = state.session(session_id)?;
    let history = Arc::clone(&state.history);
    let started_at = SystemTime::now();
    let entry_sql = sql.clone();
    let record = move |outcome: Outcome| {
        let (duration, rows, status, error) = match outcome {
            Outcome::Rows { elapsed } => (elapsed, None, HistoryStatus::Ok, None),
            Outcome::Affected { rows, elapsed } => (elapsed, Some(rows), HistoryStatus::Ok, None),
            Outcome::Failed {
                error: Error::Cancelled,
                elapsed,
            } => (elapsed, None, HistoryStatus::Cancelled, None),
            Outcome::Failed { error, elapsed } => {
                (elapsed, None, HistoryStatus::Error, Some(error.to_string()))
            }
        };
        if !keep {
            return;
        }
        if let Ok(history) = &*lock(&history) {
            // History is best effort: a failed write must not affect the query.
            let _ = history.record(&NewHistoryEntry {
                connection_id: session.connection_id,
                sql: entry_sql,
                started_at,
                duration,
                rows,
                status,
                error,
                origin: None,
            });
        }
    };
    let job = Job::start(session_id, conn, sql, limit, on_event, record);
    let job_id = state.next_id();
    lock(&state.jobs).insert(job_id, job);
    Ok(job_id)
}

/// Rows `offset..offset + limit` of a result, rendered as text, as an Arrow IPC stream.
/// Returns fewer rows when they have not been fetched yet.
#[tauri::command]
pub fn fetch_rows(
    state: State<'_, AppState>,
    job_id: u64,
    offset: usize,
    limit: usize,
) -> CommandResult<Response> {
    let job = state.job(job_id)?;
    job.request(offset.saturating_add(limit));
    let (schema, batches) = job
        .window(offset, limit)
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    // A store that cannot be read leaves the plain formats.
    let format = state
        .with_settings(|s| Ok(DisplayFormat::from_settings(s)))
        .unwrap_or_default();
    Ok(Response::new(display::to_ipc(&schema, &batches, &format)?))
}

#[tauri::command]
pub fn fetch_all(state: State<'_, AppState>, job_id: u64) -> CommandResult<()> {
    state.job(job_id)?.fetch_all();
    Ok(())
}

#[tauri::command]
pub async fn cancel_query(state: State<'_, AppState>, job_id: u64) -> CommandResult<()> {
    let job = state.job(job_id)?;
    job.cancel().await
}

/// Releases a result's buffers; an unfinished query is cancelled.
#[tauri::command]
pub fn close_query(state: State<'_, AppState>, job_id: u64) {
    if let Some(job) = lock(&state.jobs).remove(&job_id) {
        job.close();
    }
}

/// Stops fetching every unfinished result of a session, so it can run another statement.
pub(super) fn stop_jobs(state: &AppState, session_id: u64) {
    let jobs: Vec<_> = lock(&state.jobs)
        .values()
        .filter(|j| j.session_id == session_id)
        .cloned()
        .collect();
    for job in jobs {
        job.stop();
    }
}
