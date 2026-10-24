//! Keep-alive of idle sessions: a light query that measures the latency and reopens a
//! connection the server dropped, so the next statement does not find it gone.

use std::time::Instant;

use futures::TryStreamExt;
use platypus_core::{ExecOptions, ExecResult};
use serde::Serialize;
use tauri::State;

use super::{AppState, lock};
use crate::error::CommandResult;

#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Health {
    /// Answered in `latency_ms`; `reconnected` when the connection had to be reopened.
    Ok { latency_ms: u64, reconnected: bool },
    /// A result is still being fetched on the session: not disturbed.
    Busy,
}

#[tauri::command]
pub async fn ping_session(state: State<'_, AppState>, session_id: u64) -> CommandResult<Health> {
    let busy = lock(&state.jobs)
        .values()
        .any(|j| j.session_id == session_id && !j.is_done());
    if busy {
        return Ok(Health::Busy);
    }
    let was_closed = state.session(session_id)?.conn.is_closed();
    // Reopens a dropped connection; an open transaction lost on the way is an error.
    state.live(session_id).await?;
    // Past the query log: the keep-alive only records its round trip there.
    let session = state.session(session_id)?;
    let conn = session.raw;
    let start = Instant::now();
    let pinged = match conn.execute("SELECT 1", &ExecOptions::default()).await {
        Ok(ExecResult::Rows(stream)) => stream.try_for_each(|_| async { Ok(()) }).await,
        Ok(ExecResult::Affected { .. }) => Ok(()),
        Err(e) => Err(e),
    };
    let reconnected = match pinged {
        // Dropped since the last statement: the ping found out, reopen it now.
        Err(_) if conn.is_closed() => {
            state.live(session_id).await?;
            true
        }
        // Answered, even with an error (PostgreSQL refuses statements in a transaction a
        // failed one aborted): the connection is alive.
        _ => was_closed,
    };
    let elapsed = start.elapsed();
    if !reconnected {
        state
            .query_log
            .set_latency(&session.label, elapsed.as_secs_f64() * 1000.0);
    }
    let latency_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    Ok(Health::Ok {
        latency_ms,
        reconnected,
    })
}
