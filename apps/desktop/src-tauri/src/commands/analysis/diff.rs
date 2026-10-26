//! "Compare with previous run": the loaded rows of the result a statement replaced, one copy
//! per session (so per tab), compared with the new result.

use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use platypus_local::diff::{self, Diff, Table};
use tauri::State;

use super::{Loaded, loaded};
use crate::commands::summary::blocking;
use crate::commands::{AppState, lock};
use crate::display;
use crate::error::{CommandError, CommandResult};

/// The rows of a session's previous run, sharing the old result's buffers.
#[derive(Debug, Clone)]
pub struct PreviousRun {
    schema: SchemaRef,
    batches: Vec<RecordBatch>,
    /// They were all of its rows.
    complete: bool,
}

/// Before the statement of result `job_id` runs again: keeps the rows it loaded, in place of
/// what the session kept before.
#[tauri::command]
pub fn keep_previous_run(
    state: State<'_, AppState>,
    session_id: u64,
    job_id: u64,
) -> CommandResult<()> {
    let rows = loaded(&state, job_id)?;
    let run = PreviousRun {
        schema: rows.schema,
        batches: rows.batches,
        complete: rows.complete,
    };
    lock(&state.previous_runs).insert(session_id, run);
    Ok(())
}

/// The session runs another statement: the copy goes.
#[tauri::command]
pub fn forget_previous_run(state: State<'_, AppState>, session_id: u64) {
    forget(&state, session_id);
}

/// Drops what session `session_id` kept, e.g. when it closes.
pub(in crate::commands) fn forget(state: &AppState, session_id: u64) {
    lock(&state.previous_runs).remove(&session_id);
}

/// What changed from the session's previous run to result `job_id`: rows matched by the `key`
/// columns when given, else by all their values. Over the rows each run loaded: see
/// `platypus_local::diff` for what a run not loaded in full leaves out.
#[tauri::command]
pub async fn diff_previous(
    state: State<'_, AppState>,
    session_id: u64,
    job_id: u64,
    key: Option<Vec<usize>>,
) -> CommandResult<Diff> {
    let rows = loaded(&state, job_id)?;
    let previous = previous_of(&state, session_id)?;
    blocking(move || compare(&previous, &rows, key.as_deref())).await
}

fn previous_of(state: &AppState, session_id: u64) -> CommandResult<PreviousRun> {
    lock(&state.previous_runs)
        .get(&session_id)
        .cloned()
        .ok_or_else(|| {
            CommandError::new(
                "no_previous",
                "There is no previous run of this statement to compare with.",
            )
        })
}

fn compare(previous: &PreviousRun, rows: &Loaded, key: Option<&[usize]>) -> CommandResult<Diff> {
    let before = Table {
        schema: &previous.schema,
        batches: &previous.batches,
        complete: previous.complete,
    };
    let after = Table {
        schema: &rows.schema,
        batches: &rows.batches,
        complete: rows.complete,
    };
    Ok(diff::diff(before, after, key, &display::format_options())?)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_schema::{DataType, Field, Schema};

    use super::*;
    use crate::commands::analysis::error_kind;

    fn schema(column: &str) -> SchemaRef {
        Arc::new(Schema::new(vec![Field::new(
            column,
            DataType::Int32,
            false,
        )]))
    }

    #[test]
    fn nothing_to_compare_without_a_previous_run() {
        let state = AppState::for_tests();
        let Err(e) = previous_of(&state, 1) else {
            panic!("nothing was kept")
        };
        assert_eq!(error_kind(&e), "no_previous");
    }

    #[test]
    fn results_with_other_columns_are_not_compared() {
        let previous = PreviousRun {
            schema: schema("a"),
            batches: Vec::new(),
            complete: true,
        };
        let rows = Loaded {
            schema: schema("b"),
            batches: Vec::new(),
            complete: true,
        };
        let Err(e) = compare(&previous, &rows, None) else {
            panic!("the columns differ")
        };
        assert_eq!(error_kind(&e), "columns_differ");
    }

    #[test]
    fn a_kept_run_goes_with_its_session() {
        let state = AppState::for_tests();
        let run = PreviousRun {
            schema: schema("a"),
            batches: Vec::new(),
            complete: true,
        };
        lock(&state.previous_runs).insert(3, run);
        assert!(previous_of(&state, 3).is_ok());
        forget(&state, 3);
        assert!(previous_of(&state, 3).is_err());
    }
}
