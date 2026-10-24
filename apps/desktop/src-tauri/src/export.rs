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
use platypus_core::{ConnectionConfig, Error, ExecOptions, ExecResult};
use platypus_export::{ExportError, ExportOptions, Exporter};
use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tokio::sync::mpsc;

use crate::error::CommandError;
use crate::querylog::{Logged, QueryLog};

const BATCH_SIZE: usize = 8192;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExportEvent {
    Progress { rows: u64 },
    Done { rows: u64, elapsed_ms: u64 },
    Failed { error: CommandError },
    Cancelled,
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
        let export = Arc::new(Self {
            events,
            task: Mutex::new(None),
            over: AtomicBool::new(false),
        });
        let task = Arc::clone(&export);
        let handle = tauri::async_runtime::spawn(async move {
            let started = Instant::now();
            let event = match task.run(&config, &label, &log, &sql, path, options).await {
                Ok(rows) => ExportEvent::Done {
                    rows,
                    elapsed_ms: millis(started),
                },
                Err(error) => ExportEvent::Failed { error },
            };
            task.finish(event);
        });
        *lock(&export.task) = Some(handle);
        export
    }

    fn finish(&self, event: ExportEvent) {
        if !self.over.swap(true, Ordering::AcqRel) {
            let _ = self.events.send(event);
        }
    }

    async fn run(
        &self,
        config: &ConnectionConfig,
        label: &str,
        log: &Arc<QueryLog>,
        sql: &str,
        path: PathBuf,
        options: ExportOptions,
    ) -> Result<u64, CommandError> {
        let conn = Logged::wrap(crate::commands::connect(config).await?, log, label);
        let options_exec = ExecOptions {
            batch_size: BATCH_SIZE,
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
                let _ = self.events.send(ExportEvent::Progress { rows });
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

    /// Stops the export and deletes the partial file.
    pub fn cancel(&self) {
        if let Some(task) = lock(&self.task).take() {
            task.abort();
        }
        self.finish(ExportEvent::Cancelled);
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
