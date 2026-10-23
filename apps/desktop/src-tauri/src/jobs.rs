//! Query jobs: run a statement in the background and buffer its result for the grid.
//!
//! Rows are fetched on demand. The job reads from the server until it holds `target` rows,
//! then pauses (the driver applies backpressure, so the server waits too); scrolling the
//! grid raises the target. A memory cap stops runaway results.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use futures::StreamExt;
use platypus_core::{CancelHandle, Connection, ExecOptions, ExecResult, META_DB_TYPE};
use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tokio::sync::watch;

use crate::error::CommandError;

/// Rows fetched ahead of what the grid has asked for.
pub const PREFETCH_ROWS: usize = 10_000;
/// Buffered Arrow data per result before fetching stops.
pub const MEMORY_LIMIT_BYTES: usize = 512 * 1024 * 1024;
const BATCH_SIZE: usize = 2048;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    name: String,
    /// Original database type, e.g. `numeric(12,2)`.
    db_type: String,
    /// Right-align in the grid.
    numeric: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueryEvent {
    /// The statement returns rows; `elapsed_ms` is the time to the first response.
    Started {
        columns: Vec<ColumnInfo>,
        elapsed_ms: u64,
    },
    Progress {
        rows: usize,
    },
    /// No more rows will arrive. `complete` is false when the memory cap stopped fetching.
    Finished {
        rows: usize,
        elapsed_ms: u64,
        complete: bool,
    },
    Affected {
        rows: u64,
        elapsed_ms: u64,
    },
    Failed {
        error: CommandError,
    },
}

#[derive(Debug, Default)]
struct Buffer {
    schema: Option<SchemaRef>,
    batches: Vec<RecordBatch>,
    /// Row index of the first row of each batch.
    starts: Vec<usize>,
    rows: usize,
    bytes: usize,
}

impl Buffer {
    fn push(&mut self, batch: RecordBatch) {
        self.starts.push(self.rows);
        self.rows += batch.num_rows();
        self.bytes += batch.get_array_memory_size();
        self.batches.push(batch);
    }

    /// Zero-copy slices covering rows `offset..offset + limit` (clamped to what is buffered).
    fn window(&self, offset: usize, limit: usize) -> Vec<RecordBatch> {
        let end = offset.saturating_add(limit).min(self.rows);
        if offset >= end {
            return Vec::new();
        }
        // Last batch starting at or before `offset`.
        let first = self.starts.partition_point(|&start| start <= offset) - 1;
        self.batches[first..]
            .iter()
            .zip(&self.starts[first..])
            .take_while(|&(_, &start)| start < end)
            .map(|(batch, &start)| {
                let from = offset.saturating_sub(start);
                let to = (end - start).min(batch.num_rows());
                batch.slice(from, to - from)
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct Job {
    buffer: Mutex<Buffer>,
    target: watch::Sender<usize>,
    cancel: CancelHandle,
    task: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "query durations fit in u64 milliseconds"
)]
fn millis(since: Instant) -> u64 {
    since.elapsed().as_millis() as u64
}

impl Job {
    pub fn start(conn: Arc<dyn Connection>, sql: String, events: Channel<QueryEvent>) -> Arc<Self> {
        let job = Arc::new(Self {
            buffer: Mutex::new(Buffer::default()),
            target: watch::Sender::new(PREFETCH_ROWS),
            cancel: conn.cancel_handle(),
            task: Mutex::new(None),
        });
        let task = tauri::async_runtime::spawn(Arc::clone(&job).run(conn, sql, events));
        *lock(&job.task) = Some(task);
        job
    }

    async fn run(
        self: Arc<Self>,
        conn: Arc<dyn Connection>,
        sql: String,
        events: Channel<QueryEvent>,
    ) {
        // A closed channel only means the UI went away; the job is closed separately.
        let send = |event| {
            let _ = events.send(event);
        };
        let started = Instant::now();
        let options = ExecOptions {
            batch_size: BATCH_SIZE,
        };
        let mut stream = match conn.execute(&sql, &options).await {
            Err(e) => return send(QueryEvent::Failed { error: e.into() }),
            Ok(ExecResult::Affected { rows }) => {
                return send(QueryEvent::Affected {
                    rows,
                    elapsed_ms: millis(started),
                });
            }
            Ok(ExecResult::Rows(stream)) => stream,
        };

        let schema = stream.schema();
        lock(&self.buffer).schema = Some(Arc::clone(&schema));
        let columns = schema
            .fields()
            .iter()
            .map(|f| ColumnInfo {
                name: f.name().clone(),
                db_type: f.metadata().get(META_DB_TYPE).cloned().unwrap_or_default(),
                numeric: f.data_type().is_numeric(),
            })
            .collect();
        send(QueryEvent::Started {
            columns,
            elapsed_ms: millis(started),
        });

        let mut demand = self.target.subscribe();
        let mut reported = 0;
        let mut last_report = Instant::now();
        let mut complete = true;
        loop {
            let rows = lock(&self.buffer).rows;
            if !demand.borrow().gt(&rows) {
                if rows != reported {
                    send(QueryEvent::Progress { rows });
                    reported = rows;
                }
                if demand.wait_for(|target| *target > rows).await.is_err() {
                    break;
                }
            }
            match stream.next().await {
                None => break,
                Some(Err(e)) => return send(QueryEvent::Failed { error: e.into() }),
                Some(Ok(batch)) => {
                    let mut buffer = lock(&self.buffer);
                    buffer.push(batch);
                    if buffer.bytes > MEMORY_LIMIT_BYTES {
                        complete = false;
                        break;
                    }
                }
            }
            if last_report.elapsed() >= PROGRESS_INTERVAL {
                reported = lock(&self.buffer).rows;
                send(QueryEvent::Progress { rows: reported });
                last_report = Instant::now();
            }
        }
        // Dropping an unfinished stream cancels the rest of the query server-side.
        drop(stream);
        let rows = lock(&self.buffer).rows;
        send(QueryEvent::Finished {
            rows,
            elapsed_ms: millis(started),
            complete,
        });
    }

    /// Asks the job to hold at least `rows` rows (plus prefetch).
    pub fn request(&self, rows: usize) {
        let wanted = rows.saturating_add(PREFETCH_ROWS);
        self.target.send_if_modified(|target| {
            let raise = wanted > *target;
            if raise {
                *target = wanted;
            }
            raise
        });
    }

    pub fn fetch_all(&self) {
        self.target.send_replace(usize::MAX);
    }

    /// The buffered rows in `offset..offset + limit`, as slices of the stored batches.
    pub fn window(&self, offset: usize, limit: usize) -> Option<(SchemaRef, Vec<RecordBatch>)> {
        let buffer = lock(&self.buffer);
        let schema = Arc::clone(buffer.schema.as_ref()?);
        Some((schema, buffer.window(offset, limit)))
    }

    pub async fn cancel(&self) -> Result<(), CommandError> {
        self.cancel.cancel().await.map_err(Into::into)
    }

    /// Stops the background task; its stream is dropped, which cancels the query.
    pub fn close(&self) {
        if let Some(task) = lock(&self.task).take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use arrow_array::{Array, Int32Array};
    use arrow_schema::{DataType, Field, Schema};

    use super::*;

    /// Batches of the given sizes holding consecutive integers from 0.
    fn buffer(sizes: &[i32]) -> Buffer {
        let schema = Arc::new(Schema::new(vec![Field::new("n", DataType::Int32, false)]));
        let mut buffer = Buffer {
            schema: Some(Arc::clone(&schema)),
            ..Buffer::default()
        };
        let mut next = 0;
        for &size in sizes {
            let values = Int32Array::from_iter_values(next..next + size);
            next += size;
            buffer.push(RecordBatch::try_new(Arc::clone(&schema), vec![Arc::new(values)]).unwrap());
        }
        buffer
    }

    fn values(batches: &[RecordBatch]) -> Vec<i32> {
        batches
            .iter()
            .flat_map(|b| {
                let col = b.column(0).as_any().downcast_ref::<Int32Array>().unwrap();
                (0..col.len()).map(|i| col.value(i)).collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn windows_span_batch_boundaries() {
        let b = buffer(&[3, 4, 2]); // rows 0..9
        assert_eq!(values(&b.window(0, 3)), [0, 1, 2]);
        assert_eq!(values(&b.window(2, 3)), [2, 3, 4]);
        assert_eq!(values(&b.window(1, 100)), [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(values(&b.window(7, 2)), [7, 8]);
        assert_eq!(b.window(3, 4).len(), 1, "exactly one batch");
    }

    #[test]
    fn windows_outside_the_buffer_are_empty() {
        let b = buffer(&[3]);
        assert_eq!(values(&b.window(3, 10)), Vec::<i32>::new());
        assert_eq!(values(&b.window(100, 10)), Vec::<i32>::new());
        assert_eq!(values(&b.window(1, 0)), Vec::<i32>::new());
        assert_eq!(values(&buffer(&[]).window(0, 10)), Vec::<i32>::new());
    }
}
