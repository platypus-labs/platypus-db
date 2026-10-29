//! Data analysis of results: column profiles, charts, SQL on results and the comparison with
//! the previous run. `platypus_local` computes; these commands take the rows from the result's
//! job and do the work off the async runtime.

pub mod chart;
pub mod diff;
pub mod sql;
pub mod stats;
pub mod table_profile;

use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;

use crate::commands::AppState;
use crate::error::{CommandError, CommandResult};

/// A result's rows as loaded so far.
pub(crate) struct Loaded {
    pub schema: SchemaRef,
    pub batches: Vec<RecordBatch>,
    /// They are all of the result's rows.
    pub complete: bool,
}

/// The result was closed (its tab closed or its statement ran again) or never existed.
pub(crate) fn gone() -> CommandError {
    CommandError::new(
        "result_gone",
        "The result is no longer available: run the query again.",
    )
}

/// The rows result `job_id` has loaded.
pub(crate) fn loaded(state: &AppState, job_id: u64) -> CommandResult<Loaded> {
    let job = state.job(job_id).map_err(|_| gone())?;
    let (schema, batches) = job
        .snapshot()
        .ok_or_else(|| CommandError::new("not_ready", "The result has no rows yet."))?;
    Ok(Loaded {
        schema,
        batches,
        complete: job.is_complete(),
    })
}

/// The `kind` the UI branches on, for tests.
#[cfg(test)]
pub(crate) fn error_kind(e: &CommandError) -> String {
    serde_json::to_value(e).unwrap()["kind"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::AppState;

    #[test]
    fn a_closed_result_is_gone() {
        let state = AppState::for_tests();
        let Err(e) = loaded(&state, 42) else {
            panic!("there is no job 42")
        };
        assert_eq!(error_kind(&e), "result_gone");
    }
}
