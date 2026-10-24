//! Queries inside a transaction, and paged reads (a result grid), run through a
//! server-side cursor.
//!
//! Inside a transaction a cancel would abort it, so queries are declared as cursors and
//! read with `FETCH`: abandoning the result just closes the cursor. A paged read outside a
//! transaction gets one of its own, committed when the cursor is closed: the server sends
//! only the rows asked for, so an abandoned result ends at once instead of being drained
//! (seconds over a slow link).

use std::sync::Arc;

use arrow_array::RecordBatch;
use arrow_schema::Schema;
use futures::StreamExt;
use platypus_core::{Error, ExecOptions, ExecResult, RecordBatchStream, Result, leading_words};
use platypus_sql::Dialect;
use tokio::sync::OwnedMutexGuard;
use tokio_postgres::{Client, Statement};

use crate::columns::{ColumnBuilder, column_builder};
use crate::{append_row, map_error, no_params};

/// Whether `sql` can be declared as a cursor: a plain query with no side effects
/// (`DECLARE` rejects data-modifying statements, and a failed `DECLARE` would abort the
/// transaction).
pub fn eligible(sql: &str) -> bool {
    let first = leading_words(sql, 1);
    matches!(
        first.first().map(String::as_str),
        Some("SELECT" | "WITH" | "VALUES" | "TABLE")
    ) && platypus_sql::is_read_only(sql, Dialect::Postgres)
}

/// Declares `sql` as cursor `name`. `own`: no transaction is open, so the cursor gets one
/// of its own.
pub async fn open(
    client: Arc<Client>,
    name: &str,
    sql: &str,
    options: &ExecOptions,
    guard: OwnedMutexGuard<()>,
    own: bool,
) -> Result<ExecResult> {
    let query = sql.trim().trim_end_matches(';');
    let begin = if own { "BEGIN; " } else { "" };
    let batch_size = options.batch_size.max(1);
    let declare = format!("{begin}DECLARE {name} NO SCROLL CURSOR FOR ");
    let declared = async {
        client.batch_execute(&format!("{declare}{query}")).await?;
        client
            .prepare(&format!("FETCH {batch_size} FROM {name}"))
            .await
    };
    let fetch = match declared.await {
        Ok(fetch) => fetch,
        Err(e) => {
            if own {
                let _ = client.batch_execute("ROLLBACK").await;
            }
            return Err(in_query(map_error(e), sql, &declare));
        }
    };
    let (fields, builders): (Vec<_>, Vec<_>) = fetch
        .columns()
        .iter()
        .map(column_builder)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .unzip();
    let schema = Arc::new(Schema::new(fields));
    let state = CursorState {
        client,
        fetch,
        name: name.to_owned(),
        builders,
        schema: Arc::clone(&schema),
        batch_size,
        own,
        guard: Some(guard),
    };
    // The query runs on the first FETCH: read it here, so the statement returns when its
    // first rows do (the time to the first response, slow-query marks) and fails as one.
    let stream = match next_batch(state).await? {
        None => futures::stream::empty().boxed(),
        Some((first, state)) => futures::stream::once(async { Ok(first) })
            .chain(futures::stream::try_unfold(state, next_batch))
            .boxed(),
    };
    Ok(ExecResult::Rows(RecordBatchStream::new(schema, stream)))
}

/// An error of `{declare}{query}` with its position in `sql`, the statement as typed: the
/// server counts characters from the start of what it was sent.
fn in_query(mut e: Error, sql: &str, declare: &str) -> Error {
    if let Error::Database(db) = &mut e {
        let trimmed = sql.len() - sql.trim_start().len();
        let shift = declare.chars().count() - sql[..trimmed].chars().count();
        db.position = db
            .position
            .and_then(|p| usize::try_from(p).ok()?.checked_sub(shift))
            .filter(|&p| p > 0)
            .and_then(|p| u32::try_from(p).ok());
    }
    e
}

struct CursorState {
    client: Arc<Client>,
    fetch: Statement,
    name: String,
    builders: Vec<ColumnBuilder>,
    schema: Arc<Schema>,
    batch_size: usize,
    /// The cursor has a transaction of its own, ended with it.
    own: bool,
    /// `None` once the cursor is closed.
    guard: Option<OwnedMutexGuard<()>>,
}

impl CursorState {
    /// What closes the cursor. Its own transaction is committed, not rolled back: a query
    /// can still change data through the functions it calls.
    fn close_sql(&self) -> String {
        let end = if self.own { "; COMMIT" } else { "" };
        format!("CLOSE {}{end}", self.name)
    }

    async fn close(&mut self) {
        if self.guard.is_some() {
            let _ = self.client.batch_execute(&self.close_sql()).await;
            self.guard = None;
        }
    }

    /// After a failed `FETCH`: the transaction is aborted and the cursor gone with it; its
    /// own transaction is ended so the session can run statements again.
    async fn failed(&mut self, e: tokio_postgres::Error) -> platypus_core::Error {
        if self.own {
            let _ = self.client.batch_execute("ROLLBACK").await;
        }
        self.guard = None;
        map_error(e)
    }
}

async fn next_batch(mut state: CursorState) -> Result<Option<(RecordBatch, CursorState)>> {
    if state.guard.is_none() {
        return Ok(None);
    }
    let rows = match state.client.query_raw(&state.fetch, no_params()).await {
        Ok(rows) => rows,
        Err(e) => return Err(state.failed(e).await),
    };
    let mut rows = std::pin::pin!(rows);
    let mut len = 0;
    while let Some(row) = rows.next().await {
        let row = match row {
            Ok(row) => row,
            Err(e) => return Err(state.failed(e).await),
        };
        append_row(&mut state.builders, &state.schema, &row)?;
        len += 1;
    }
    if len < state.batch_size {
        state.close().await;
    }
    if len == 0 {
        return Ok(None);
    }
    let columns = state
        .builders
        .iter_mut()
        .map(ColumnBuilder::finish)
        .collect();
    let batch = RecordBatch::try_new(Arc::clone(&state.schema), columns)?;
    Ok(Some((batch, state)))
}

impl Drop for CursorState {
    /// An abandoned cursor is closed in the background; the connection stays busy until
    /// then, so the next statement cannot interleave with the `CLOSE`.
    fn drop(&mut self) {
        let Some(guard) = self.guard.take() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let client = Arc::clone(&self.client);
        let close = self.close_sql();
        runtime.spawn(async move {
            let _ = client.batch_execute(&close).await;
            drop(guard);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_only_plain_queries() {
        assert!(eligible("select * from t"));
        assert!(eligible("  with x as (select 1) select * from x"));
        assert!(eligible("values (1), (2)"));
        assert!(!eligible(
            "with d as (delete from t returning *) select * from d"
        ));
        assert!(!eligible("insert into t values (1) returning id"));
        assert!(!eligible("explain select 1"));
        assert!(!eligible("show search_path"));
        assert!(!eligible("select * from t for update"));
    }
}
