//! Running one statement: the result's first page decides whether it has rows, and the rest
//! are read page by page as the reader asks for them, with the driver's paging state.

use std::sync::Arc;

use futures::StreamExt;
use futures::stream::{self, BoxStream};
use platypus_core::{Error, ExecOptions, ExecResult, RecordBatchStream, Result};
use scylla::errors::ExecutionError;
use scylla::response::query_result::QueryResult;
use scylla::response::{PagingState, PagingStateResponse};
use scylla::statement::Statement;
use scylla::value::Row;
use tokio::sync::watch;

use crate::columns::Layout;
use crate::cql::{self, Kind};
use crate::error;
use crate::session::Shared;

/// Runs `text`, a single CQL statement.
pub async fn run(shared: &Arc<Shared>, text: &str, options: &ExecOptions) -> Result<ExecResult> {
    if let Kind::Use(keyspace) = cql::kind(text) {
        return use_keyspace(shared, &keyspace.name).await;
    }
    let mut statement = Statement::new(text);
    statement.set_page_size(i32::try_from(options.batch_size.max(1)).unwrap_or(i32::MAX));
    let mut cancel = shared.cancel.subscribe();
    let (result, state) = fetch(shared, &mut cancel, &statement, PagingState::start()).await?;
    shared.warn(result.warnings());
    if !result.is_rows() {
        // Writes and schema changes report nothing about the rows they touched.
        return Ok(ExecResult::Affected { rows: 0 });
    }
    let rows = result
        .into_rows_result()
        .map_err(|e| error::from_driver(&e))?;
    let layout = Layout::of(rows.column_specs().iter());
    let first = decode(&rows)?;
    let pager = Pager {
        shared: Arc::clone(shared),
        statement,
        state,
        layout: layout.clone(),
        cancel,
        first: Some(first),
    };
    let batches: BoxStream<'static, Result<_>> =
        stream::try_unfold(pager, |mut pager| async move {
            Ok(pager.next_batch().await?.map(|batch| (batch, pager)))
        })
        .boxed();
    Ok(ExecResult::Rows(RecordBatchStream::new(
        layout.schema(),
        batches,
    )))
}

/// `USE keyspace`: the driver holds the keyspace of the whole session.
async fn use_keyspace(shared: &Shared, name: &str) -> Result<ExecResult> {
    let mut cancel = shared.cancel.subscribe();
    shared
        .guarded(&mut cancel, async {
            shared
                .session
                .use_keyspace(name, true)
                .await
                .map_err(|e| error::from_driver(&e))
        })
        .await?;
    if let Ok(mut current) = shared.keyspace.lock() {
        *current = Some(name.to_owned());
    }
    Ok(ExecResult::Affected { rows: 0 })
}

/// One page of `statement` from `state`.
async fn fetch(
    shared: &Shared,
    cancel: &mut watch::Receiver<u64>,
    statement: &Statement,
    state: PagingState,
) -> Result<(QueryResult, PagingStateResponse)> {
    shared
        .guarded(cancel, async {
            shared
                .session
                .query_single_page(statement.clone(), (), state)
                .await
                .map_err(|e: ExecutionError| error::from_driver(&e))
        })
        .await
}

/// The rows of a page.
fn decode(rows: &scylla::response::query_result::QueryRowsResult) -> Result<Vec<Row>> {
    rows.rows::<Row>()
        .map_err(|e| error::from_driver(&e))?
        .map(|row| row.map_err(|e| Error::Other(format!("cannot read a row: {e}"))))
        .collect()
}

/// The pages after the first, fetched one at a time.
struct Pager {
    shared: Arc<Shared>,
    statement: Statement,
    state: PagingStateResponse,
    layout: Layout,
    cancel: watch::Receiver<u64>,
    /// The first page, read before the stream was handed out.
    first: Option<Vec<Row>>,
}

impl Pager {
    /// The next page that has rows, `None` after the last. An empty page is skipped: the
    /// server returns them when a filter finds nothing in the part of the table it read.
    async fn next_batch(&mut self) -> Result<Option<arrow_array::RecordBatch>> {
        if let Some(rows) = self.first.take()
            && !rows.is_empty()
        {
            return self.layout.batch(&rows).map(Some);
        }
        while let PagingStateResponse::HasMorePages { state } = &self.state {
            let (result, next) = fetch(
                &self.shared,
                &mut self.cancel,
                &self.statement,
                state.clone(),
            )
            .await?;
            self.state = next;
            let rows = result
                .into_rows_result()
                .map_err(|e| error::from_driver(&e))?;
            let rows = decode(&rows)?;
            if !rows.is_empty() {
                return self.layout.batch(&rows).map(Some);
            }
        }
        Ok(None)
    }
}
