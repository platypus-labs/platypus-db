//! "Profile data": every column of a table (or a view) described from its rows, read on the
//! tab's own session and analysed on this machine by `platypus_local::table_profile`.

use arrow_array::RecordBatch;
use futures::{Stream, StreamExt};
use platypus_core::{ExecOptions, ExecResult};
use platypus_local::table_profile::{self as profiler, TableProfile};
use platypus_sql::Dialect;
use serde::Serialize;
use tauri::State;

use crate::commands::AppState;
use crate::commands::summary::blocking;
use crate::display;
use crate::error::CommandResult;
use crate::jobs::MEMORY_LIMIT_BYTES;

/// Why the profile covers fewer rows than the table has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cut {
    /// The table has more rows than `max_rows`: the first ones were read.
    Rows,
    /// Reading stopped at the memory limit, the same as a query result's.
    Memory,
}

#[derive(Debug, Serialize)]
pub struct TableProfileReply {
    #[serde(flatten)]
    profile: TableProfile,
    /// Set when not every row of the table was read.
    cut: Option<Cut>,
}

/// Profiles the first `max_rows` rows (all of them for `None`) of `schema`.`table`.
#[tauri::command]
pub async fn table_profile(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    table: String,
    max_rows: Option<usize>,
) -> CommandResult<TableProfileReply> {
    let dialect = Dialect::from(state.session(session_id)?.config.driver);
    let conn = state.live(session_id).await?;
    let qualified = dialect.qualified(Some(&schema), &table);
    let select = format!("SELECT * FROM {qualified}");
    // One row more than asked, to tell a table of exactly `max_rows` rows from a longer one.
    let sql = match max_rows {
        Some(max) => dialect.limited(&select, max as u64 + 1),
        None => select,
    };
    let ExecResult::Rows(stream) = conn.execute(&sql, &ExecOptions::default()).await? else {
        return Err(platypus_core::Error::Other(format!("{qualified} returned no columns")).into());
    };
    let schema = stream.schema();
    let (batches, cut) = read_rows(stream, max_rows, MEMORY_LIMIT_BYTES).await?;
    let profile = blocking(move || {
        let options = display::format_options();
        Ok(profiler::profile_table(&schema, &batches, &options)?)
    })
    .await?;
    Ok(TableProfileReply { profile, cut })
}

/// Collects batches of `stream` up to `max_rows` rows and `memory` bytes; the cut tells which
/// limit ended it, if one did.
async fn read_rows(
    mut stream: impl Stream<Item = platypus_core::Result<RecordBatch>> + Unpin,
    max_rows: Option<usize>,
    memory: usize,
) -> CommandResult<(Vec<RecordBatch>, Option<Cut>)> {
    let mut batches = Vec::new();
    let (mut rows, mut bytes) = (0, 0);
    while let Some(batch) = stream.next().await {
        let batch = batch?;
        let room = max_rows.map_or(usize::MAX, |max| max - rows);
        if batch.num_rows() > room {
            batches.push(batch.slice(0, room));
            return Ok((batches, Some(Cut::Rows)));
        }
        rows += batch.num_rows();
        bytes += batch.get_array_memory_size();
        batches.push(batch);
        if bytes > memory {
            return Ok((batches, Some(Cut::Memory)));
        }
    }
    Ok((batches, None))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::Int32Array;
    use arrow_schema::{DataType, Field, Schema};
    use futures::stream;

    use super::*;

    fn batch(n: i32) -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![Field::new("n", DataType::Int32, false)]));
        RecordBatch::try_new(schema, vec![Arc::new(Int32Array::from_iter_values(0..n))]).unwrap()
    }

    fn read(sizes: &[i32], max: Option<usize>, memory: usize) -> (usize, Option<Cut>) {
        let items: Vec<platypus_core::Result<RecordBatch>> =
            sizes.iter().map(|&n| Ok(batch(n))).collect();
        let (batches, cut) =
            tauri::async_runtime::block_on(read_rows(stream::iter(items), max, memory)).unwrap();
        (batches.iter().map(RecordBatch::num_rows).sum(), cut)
    }

    #[test]
    fn reads_everything_without_a_limit() {
        assert_eq!(read(&[3, 4], None, usize::MAX), (7, None));
        assert_eq!(read(&[], None, usize::MAX), (0, None));
    }

    #[test]
    fn cuts_at_the_row_limit_inside_a_batch() {
        // The table has one row more than the limit: the extra row is the sign it was cut.
        assert_eq!(read(&[3, 4], Some(5), usize::MAX), (5, Some(Cut::Rows)));
        assert_eq!(read(&[3, 4], Some(7), usize::MAX), (7, None));
        assert_eq!(read(&[3, 4], Some(8), usize::MAX), (7, None));
        assert_eq!(read(&[3, 4], Some(0), usize::MAX), (0, Some(Cut::Rows)));
    }

    #[test]
    fn stops_at_the_memory_limit() {
        assert_eq!(read(&[3, 4, 5], None, 1), (3, Some(Cut::Memory)));
    }

    #[test]
    fn a_failing_stream_is_an_error() {
        let items: Vec<platypus_core::Result<RecordBatch>> = vec![
            Ok(batch(1)),
            Err(platypus_core::Error::Other("boom".to_owned())),
        ];
        let result = tauri::async_runtime::block_on(read_rows(stream::iter(items), None, 1 << 30));
        assert!(result.is_err());
    }
}
