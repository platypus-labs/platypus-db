//! Query jobs: run a statement in the background and buffer its result for the grid.
//!
//! Rows are fetched on demand. The job reads from the server until it holds `target` rows,
//! then pauses (the driver applies backpressure, so the server waits too); scrolling the
//! grid raises the target. A memory cap stops runaway results.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use arrow_array::RecordBatch;
use arrow_schema::{DataType, SchemaRef};
use futures::StreamExt;
use platypus_core::{CancelHandle, Connection, ExecOptions, ExecResult, META_DB_TYPE};
use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tokio::sync::watch;

use crate::error::CommandError;

/// Rows fetched ahead of what the grid has asked for.
pub const PREFETCH_ROWS: usize = 1_000;
/// Buffered Arrow data per result before fetching stops.
pub const MEMORY_LIMIT_BYTES: usize = 512 * 1024 * 1024;
const BATCH_SIZE: usize = 500;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    name: String,
    /// Original database type, e.g. `numeric(12,2)`.
    db_type: String,
    /// Right-align in the grid.
    numeric: bool,
    /// Bytes, shown as hex.
    binary: bool,
}

impl ColumnInfo {
    fn from_schema(schema: &arrow_schema::Schema) -> Vec<Self> {
        schema
            .fields()
            .iter()
            .map(|f| Self {
                name: f.name().clone(),
                db_type: f.metadata().get(META_DB_TYPE).cloned().unwrap_or_default(),
                numeric: f.data_type().is_numeric(),
                binary: matches!(
                    f.data_type(),
                    DataType::Binary
                        | DataType::LargeBinary
                        | DataType::BinaryView
                        | DataType::FixedSizeBinary(_)
                ),
            })
            .collect()
    }
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
        /// Still reading from the server; `false` once paused until the grid asks for more.
        fetching: bool,
    },
    /// No more rows will arrive. `complete` is false when the memory cap stopped fetching.
    Finished {
        rows: usize,
        elapsed_ms: u64,
        complete: bool,
        /// Fetching was stopped to free the session (COMMIT, ROLLBACK).
        stopped: bool,
    },
    Affected {
        rows: u64,
        elapsed_ms: u64,
    },
    Failed {
        error: CommandError,
    },
}

/// Why a job stopped fetching rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Ending {
    /// Still fetching, or paused until more rows are asked for.
    Running,
    /// Every row of the result is loaded.
    Complete,
    /// The buffered rows reached `MEMORY_LIMIT_BYTES`.
    MemoryLimit,
    /// A script's row limit was reached with more rows on the server.
    RowLimit,
    /// Reading failed (or the query was cancelled).
    Failed,
    /// `stop` freed the session.
    Stopped,
    /// `close` released the result.
    Closed,
}

impl Ending {
    const ALL: [Self; 7] = [
        Self::Running,
        Self::Complete,
        Self::MemoryLimit,
        Self::RowLimit,
        Self::Failed,
        Self::Stopped,
        Self::Closed,
    ];

    fn from_u8(value: u8) -> Self {
        Self::ALL
            .get(usize::from(value))
            .copied()
            .unwrap_or(Self::Running)
    }
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

    /// Drops rows beyond `cap` from the last batch.
    fn truncate(&mut self, cap: usize) {
        if self.rows <= cap {
            return;
        }
        let (Some(last), Some(&start)) = (self.batches.last_mut(), self.starts.last()) else {
            return;
        };
        *last = last.slice(0, cap - start);
        self.rows = cap;
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

pub struct Job {
    /// The session the statement runs on.
    pub session_id: u64,
    buffer: Mutex<Buffer>,
    events: Channel<QueryEvent>,
    started: Instant,
    /// Set once the background task has sent its last event, or the job was stopped or closed.
    done: AtomicBool,
    /// An `Ending`: why fetching ended, set before `done`.
    ending: AtomicU8,
    target: watch::Sender<usize>,
    cancel: CancelHandle,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job")
            .field("session_id", &self.session_id)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
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

/// How a statement went, reported once: when it fails, when it reports affected rows, or
/// when its first rows arrive (scrolling may fetch more long after that).
#[derive(Debug)]
pub enum Outcome {
    Rows {
        elapsed: Duration,
    },
    Affected {
        rows: u64,
        elapsed: Duration,
    },
    Failed {
        error: platypus_core::Error,
        elapsed: Duration,
    },
}

impl Job {
    /// Starts `sql` in the background. With `limit`, at most that many rows are fetched and
    /// the rest of the result is cancelled (used for scripts, which must free the session).
    pub fn start(
        session_id: u64,
        conn: Arc<dyn Connection>,
        sql: String,
        limit: Option<usize>,
        events: Channel<QueryEvent>,
        on_outcome: impl FnOnce(Outcome) + Send + 'static,
    ) -> Arc<Self> {
        let job = Arc::new(Self {
            session_id,
            buffer: Mutex::new(Buffer::default()),
            events: events.clone(),
            started: Instant::now(),
            done: AtomicBool::new(false),
            ending: AtomicU8::new(Ending::Running as u8),
            target: watch::Sender::new(limit.unwrap_or(PREFETCH_ROWS)),
            cancel: conn.cancel_handle(),
            task: Mutex::new(None),
        });
        let task = Arc::clone(&job);
        let run = async move {
            Arc::clone(&task)
                .run(conn, sql, limit, events, on_outcome)
                .await;
            task.done.store(true, Ordering::Release);
        };
        *lock(&job.task) = Some(tauri::async_runtime::spawn(run));
        job
    }

    async fn run(
        self: Arc<Self>,
        conn: Arc<dyn Connection>,
        sql: String,
        limit: Option<usize>,
        events: Channel<QueryEvent>,
        on_outcome: impl FnOnce(Outcome) + Send + 'static,
    ) {
        // A closed channel only means the UI went away; the job is closed separately.
        let send = |event| {
            let _ = events.send(event);
        };
        let progress = |rows, fetching| send(QueryEvent::Progress { rows, fetching });
        let started = Instant::now();
        let options = ExecOptions {
            batch_size: BATCH_SIZE,
            paged: true,
        };
        let mut stream = match conn.execute(&sql, &options).await {
            Err(error) => {
                send(QueryEvent::Failed {
                    error: CommandError::from(&error),
                });
                return on_outcome(Outcome::Failed {
                    error,
                    elapsed: started.elapsed(),
                });
            }
            Ok(ExecResult::Affected { rows }) => {
                send(QueryEvent::Affected {
                    rows,
                    elapsed_ms: millis(started),
                });
                return on_outcome(Outcome::Affected {
                    rows,
                    elapsed: started.elapsed(),
                });
            }
            Ok(ExecResult::Rows(stream)) => stream,
        };
        on_outcome(Outcome::Rows {
            elapsed: started.elapsed(),
        });
        let cap = limit.unwrap_or(usize::MAX);

        let schema = stream.schema();
        lock(&self.buffer).schema = Some(Arc::clone(&schema));
        send(QueryEvent::Started {
            columns: ColumnInfo::from_schema(&schema),
            elapsed_ms: millis(started),
        });

        let mut demand = self.target.subscribe();
        let mut last_report = Instant::now();
        let ending = loop {
            let rows = lock(&self.buffer).rows;
            if rows >= cap {
                // Complete only if the server had nothing beyond the limit.
                break match stream.next().await {
                    None => Ending::Complete,
                    Some(_) => Ending::RowLimit,
                };
            }
            if (*demand.borrow()).min(cap) <= rows {
                progress(rows, false);
                if demand
                    .wait_for(|target| (*target).min(cap) > rows)
                    .await
                    .is_err()
                {
                    break Ending::Stopped;
                }
                progress(rows, true);
            }
            match stream.next().await {
                None => break Ending::Complete,
                Some(Err(e)) => {
                    self.end(Ending::Failed);
                    return send(QueryEvent::Failed {
                        error: CommandError::from(&e),
                    });
                }
                Some(Ok(batch)) => {
                    let mut buffer = lock(&self.buffer);
                    buffer.push(batch);
                    buffer.truncate(cap);
                    if buffer.bytes > MEMORY_LIMIT_BYTES {
                        break Ending::MemoryLimit;
                    }
                }
            }
            if last_report.elapsed() >= PROGRESS_INTERVAL {
                progress(lock(&self.buffer).rows, true);
                last_report = Instant::now();
            }
        };
        // Dropping an unfinished stream cancels the rest of the query server-side.
        drop(stream);
        let rows = lock(&self.buffer).rows;
        self.end(ending);
        send(QueryEvent::Finished {
            rows,
            elapsed_ms: millis(started),
            complete: ending == Ending::Complete,
            stopped: false,
        });
    }

    /// Records why fetching ended, unless a reason already was: the first one stays.
    fn end(&self, ending: Ending) {
        let _ = self.ending.compare_exchange(
            Ending::Running as u8,
            ending as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    /// Why fetching ended; `Running` while it goes on.
    pub fn ending(&self) -> Ending {
        Ending::from_u8(self.ending.load(Ordering::Acquire))
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

    /// How many rows are buffered so far.
    pub fn loaded_rows(&self) -> usize {
        lock(&self.buffer).rows
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

    /// The background task has sent its last event: the session is free again.
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// Every row of the result is loaded: fetching ended at the end of the rows.
    pub fn is_complete(&self) -> bool {
        self.is_done() && self.ending() == Ending::Complete
    }

    /// Fetches every row (up to the memory limit), calling `on_progress` with the rows loaded
    /// every `every`, until the job ends for any reason: the end of the rows, the memory limit,
    /// an error or a cancel, `stop` or `close`. Returns why it ended.
    pub async fn fetch_whole(&self, every: Duration, on_progress: impl Fn(usize)) -> Ending {
        self.fetch_all();
        loop {
            on_progress(self.loaded_rows());
            if self.is_done() {
                return self.ending();
            }
            tokio::time::sleep(every).await;
        }
    }

    /// Stops fetching but keeps the rows already buffered: the result stays browsable while
    /// the session is freed (for COMMIT, ROLLBACK or an export).
    pub fn stop(&self) {
        if self.done.swap(true, Ordering::AcqRel) {
            return;
        }
        self.end(Ending::Stopped);
        if let Some(task) = lock(&self.task).take() {
            task.abort();
        }
        let _ = self.events.send(QueryEvent::Finished {
            rows: lock(&self.buffer).rows,
            elapsed_ms: millis(self.started),
            complete: false,
            stopped: true,
        });
    }

    /// The buffered result, for exporting the rows already loaded.
    pub fn snapshot(&self) -> Option<(SchemaRef, Vec<RecordBatch>)> {
        let buffer = lock(&self.buffer);
        Some((Arc::clone(buffer.schema.as_ref()?), buffer.batches.clone()))
    }

    /// Stops the background task, whose stream is dropped (which cancels the query), and
    /// releases the buffered rows. Whoever still holds the job sees it `Closed`.
    pub fn close(&self) {
        self.ending.store(Ending::Closed as u8, Ordering::Release);
        self.done.store(true, Ordering::Release);
        if let Some(task) = lock(&self.task).take() {
            task.abort();
        }
        *lock(&self.buffer) = Buffer::default();
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
    fn truncates_the_last_batch_to_a_cap() {
        let mut b = buffer(&[3, 4]);
        b.truncate(5);
        assert_eq!(b.rows, 5);
        assert_eq!(values(&b.window(0, 100)), [0, 1, 2, 3, 4]);
        b.truncate(10);
        assert_eq!(b.rows, 5, "a larger cap changes nothing");
    }

    struct NoCancel;

    impl platypus_core::Canceller for NoCancel {
        fn cancel<'life0, 'async_trait>(
            &'life0 self,
        ) -> std::pin::Pin<Box<dyn Future<Output = platypus_core::Result<()>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(()) })
        }
    }

    /// A job with rows whose task never ends, like one waiting on a slow server.
    fn waiting_job() -> Arc<Job> {
        let job = Arc::new(Job {
            session_id: 1,
            buffer: Mutex::new(buffer(&[3, 4])),
            events: Channel::new(|_| Ok(())),
            started: Instant::now(),
            done: AtomicBool::new(false),
            ending: AtomicU8::new(Ending::Running as u8),
            target: watch::Sender::new(PREFETCH_ROWS),
            cancel: CancelHandle::new(NoCancel),
            task: Mutex::new(None),
        });
        *lock(&job.task) = Some(tauri::async_runtime::spawn(futures::future::pending()));
        job
    }

    #[test]
    fn closing_a_job_ends_a_whole_fetch_and_frees_its_rows() {
        let job = waiting_job();
        let fetching = Arc::clone(&job);
        let wait = tauri::async_runtime::spawn(async move {
            fetching.fetch_whole(Duration::from_millis(5), |_| ()).await
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(!wait.inner().is_finished(), "still fetching");
        job.close();
        let ending = tauri::async_runtime::block_on(async {
            tokio::time::timeout(Duration::from_secs(5), wait).await
        });
        assert_eq!(ending.expect("the fetch ends").unwrap(), Ending::Closed);
        assert_eq!(job.loaded_rows(), 0, "the rows are released");
        assert!(job.snapshot().is_none());
    }

    #[test]
    fn stopping_a_job_ends_a_whole_fetch_keeping_its_rows() {
        let job = waiting_job();
        job.stop();
        let ending = tauri::async_runtime::block_on(job.fetch_whole(Duration::ZERO, |_| ()));
        assert_eq!(ending, Ending::Stopped);
        assert_eq!(job.loaded_rows(), 7);
        assert!(!job.is_complete());
        job.close();
        assert_eq!(job.ending(), Ending::Closed);
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
