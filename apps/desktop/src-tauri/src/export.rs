//! Exports: a query re-run on its own connection and streamed to a file, or the rows a
//! result has already loaded.
//!
//! The query side reads batches while a blocking task encodes and writes them; a small
//! channel between the two keeps memory flat whatever the result size.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use arrow_array::RecordBatch;
use futures::StreamExt;
use platypus_core::{Connection, ConnectionConfig, Error, ExecOptions, ExecResult};
use platypus_transfer::{ExportError, ExportOptions, Exporter};
use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tokio::sync::mpsc;

use crate::error::CommandError;
use crate::querylog::{Logged, QueryLog};

const BATCH_SIZE: usize = 8192;
pub(crate) const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExportEvent {
    Progress {
        rows: u64,
        /// Bytes of a file read, and its size: an SQL file's run counts statements as rows.
        #[serde(skip_serializing_if = "Option::is_none")]
        bytes: Option<[u64; 2]>,
    },
    Done {
        rows: u64,
        elapsed_ms: u64,
        /// What the job left out, e.g. a foreign key the target could not add.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<String>,
    },
    Failed {
        error: CommandError,
        /// The rows done before, exactly, for a job that counts them with [`Export::done`]:
        /// the progress events, a few a second, may lag behind.
        #[serde(skip_serializing_if = "Option::is_none")]
        rows: Option<u64>,
    },
    Cancelled {
        /// As for `Failed`.
        #[serde(skip_serializing_if = "Option::is_none")]
        rows: Option<u64>,
    },
}

impl From<ExportError> for CommandError {
    fn from(e: ExportError) -> Self {
        Self::new("export", e.to_string())
    }
}

enum Message {
    Batch(RecordBatch),
    /// Everything was sent: complete the file. Without it the partial file is dropped.
    Finish,
}

pub struct Export {
    events: Channel<ExportEvent>,
    task: Mutex<Option<JoinHandle<()>>>,
    /// Set once a final event was sent.
    over: AtomicBool,
    /// Cancelling asks the work to stop instead of aborting it, so that it can clean up.
    cooperative: bool,
    stopping: AtomicBool,
    warnings: Mutex<Vec<String>>,
    /// The rows done so far, as the work last counted them.
    done: Mutex<Option<u64>>,
}

impl std::fmt::Debug for Export {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Export")
            .field("over", &self.over)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "export durations fit in u64 milliseconds"
)]
fn millis(since: Instant) -> u64 {
    since.elapsed().as_millis() as u64
}

impl Export {
    /// Runs `sql` on a new connection and writes its rows to `path`.
    pub fn start(
        (config, label, log): (ConnectionConfig, String, Arc<QueryLog>),
        sql: String,
        path: PathBuf,
        options: ExportOptions,
        events: Channel<ExportEvent>,
    ) -> Arc<Self> {
        Self::start_many((config, label, log), vec![(sql, path, options)], events)
    }

    /// Runs each query of `files` on one new connection and writes its rows to the file next
    /// to it, one after the other.
    pub fn start_many(
        (config, label, log): (ConnectionConfig, String, Arc<QueryLog>),
        files: Vec<(String, PathBuf, ExportOptions)>,
        events: Channel<ExportEvent>,
    ) -> Arc<Self> {
        Self::spawn(events, false, |task| async move {
            let conn = Logged::wrap(
                Arc::from(crate::commands::connect(&config).await?),
                &log,
                &label,
            );
            let mut rows = 0;
            for (sql, path, options) in files {
                rows += task.write_query(&*conn, &sql, path, options, rows).await?;
            }
            Ok(rows)
        })
    }

    /// Runs `work`, a transfer of rows that reports progress on the job's events and returns
    /// how many rows it moved, reporting its outcome when it ends. `cooperative` work checks
    /// [`Self::stopping`] and returns an error once it has stopped and cleaned up.
    pub fn spawn<F>(
        events: Channel<ExportEvent>,
        cooperative: bool,
        work: impl FnOnce(Arc<Self>) -> F + Send + 'static,
    ) -> Arc<Self>
    where
        F: Future<Output = Result<u64, CommandError>> + Send + 'static,
    {
        let export = Self::new(events, cooperative);
        let task = Arc::clone(&export);
        let handle = tauri::async_runtime::spawn(async move {
            let started = Instant::now();
            let result = work(Arc::clone(&task)).await;
            task.finish(task.outcome(result, started));
        });
        *lock(&export.task) = Some(handle);
        export
    }

    fn new(events: Channel<ExportEvent>, cooperative: bool) -> Arc<Self> {
        Arc::new(Self {
            events,
            task: Mutex::new(None),
            over: AtomicBool::new(false),
            cooperative,
            stopping: AtomicBool::new(false),
            warnings: Mutex::new(Vec::new()),
            done: Mutex::new(None),
        })
    }

    /// The event that reports how the work, started at `started`, ended.
    fn outcome(&self, result: Result<u64, CommandError>, started: Instant) -> ExportEvent {
        let rows = *lock(&self.done);
        match result {
            Ok(rows) => ExportEvent::Done {
                rows,
                elapsed_ms: millis(started),
                warnings: std::mem::take(&mut *lock(&self.warnings)),
            },
            Err(_) if self.stopping() => ExportEvent::Cancelled { rows },
            Err(error) => ExportEvent::Failed { error, rows },
        }
    }

    /// Notes `rows` done so far, exactly: a failure or a stop reports them, where the
    /// progress events may lag behind.
    pub fn done(&self, rows: u64) {
        *lock(&self.done) = Some(rows);
    }

    /// Notes something the job left out, reported when it is done.
    pub fn warn(&self, warning: String) {
        lock(&self.warnings).push(warning);
    }

    /// Whether the job was cancelled.
    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }

    /// Reports `rows` moved so far.
    pub fn progress(&self, rows: u64) {
        let _ = self
            .events
            .send(ExportEvent::Progress { rows, bytes: None });
    }

    /// Reports `rows` (statements) done and `read` bytes of a file of `total`.
    pub fn progress_bytes(&self, rows: u64, read: u64, total: u64) {
        let bytes = Some([read, total]);
        let _ = self.events.send(ExportEvent::Progress { rows, bytes });
    }

    fn finish(&self, event: ExportEvent) {
        if !self.over.swap(true, Ordering::AcqRel) {
            let _ = self.events.send(event);
        }
    }

    /// Writes the rows of `sql` to `path`; progress counts from `done` rows.
    async fn write_query(
        &self,
        conn: &dyn Connection,
        sql: &str,
        path: PathBuf,
        options: ExportOptions,
        done: u64,
    ) -> Result<u64, CommandError> {
        let options_exec = ExecOptions {
            batch_size: BATCH_SIZE,
            ..ExecOptions::default()
        };
        let ExecResult::Rows(mut stream) = conn.execute(sql, &options_exec).await? else {
            return Err(CommandError::new(
                "export",
                "the statement returned no rows to export",
            ));
        };
        let schema = stream.schema();
        let (tx, mut rx) = mpsc::channel::<Message>(4);
        let writer = tokio::task::spawn_blocking(move || -> Result<u64, ExportError> {
            let mut exporter = Exporter::create(&path, &schema, &options)?;
            while let Some(message) = rx.blocking_recv() {
                match message {
                    Message::Batch(batch) => exporter.write(&batch)?,
                    Message::Finish => return exporter.finish(),
                }
            }
            // The sender went away without `Finish`: dropping the exporter discards the file.
            Ok(exporter.rows())
        });

        let mut rows = 0;
        let mut last_report = Instant::now();
        let mut failed: Option<Error> = None;
        while let Some(batch) = stream.next().await {
            match batch {
                Ok(batch) => {
                    rows += batch.num_rows() as u64;
                    if tx.send(Message::Batch(batch)).await.is_err() {
                        break; // The writer failed; its error is reported below.
                    }
                }
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
            if last_report.elapsed() >= PROGRESS_INTERVAL {
                self.progress(done + rows);
                last_report = Instant::now();
            }
        }
        drop(stream);
        if failed.is_none() {
            let _ = tx.send(Message::Finish).await;
        }
        drop(tx);
        let written = writer
            .await
            .map_err(|e| CommandError::new("export", e.to_string()))?;
        if let Some(e) = failed {
            return Err(e.into());
        }
        Ok(written?)
    }

    /// Stops the job: an export deletes its partial file.
    pub fn cancel(&self) {
        if self.cooperative {
            self.stopping.store(true, Ordering::Release);
            return;
        }
        if let Some(task) = lock(&self.task).take() {
            task.abort();
        }
        let rows = *lock(&self.done);
        self.finish(ExportEvent::Cancelled { rows });
    }
}

/// Writes batches already in memory; meant to run on a blocking thread.
pub fn write_batches(
    path: &std::path::Path,
    schema: &arrow_schema::Schema,
    batches: &[RecordBatch],
    options: &ExportOptions,
) -> Result<u64, ExportError> {
    let mut exporter = Exporter::create(path, schema, options)?;
    for batch in batches {
        exporter.write(batch)?;
    }
    exporter.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure() -> Result<u64, CommandError> {
        Err(CommandError::new("script", "Statement 3 failed"))
    }

    #[test]
    fn a_failure_or_a_stop_reports_the_rows_done_before_it() {
        let job = Export::new(Channel::new(|_| Ok(())), true);
        let event = job.outcome(failure(), Instant::now());
        assert_eq!(
            serde_json::to_value(&event).unwrap()["rows"],
            serde_json::Value::Null,
            "none counted: the dialog keeps what progress told"
        );
        job.done(1);
        job.done(2);
        let event = job.outcome(failure(), Instant::now());
        assert!(matches!(event, ExportEvent::Failed { rows: Some(2), .. }));
        job.stopping.store(true, Ordering::Release);
        let event = job.outcome(failure(), Instant::now());
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({ "type": "cancelled", "rows": 2 })
        );
    }
}
