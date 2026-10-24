//! The query manager's log: every statement, metadata request and transaction command the
//! app sends to a database, with how long it took and how it ended. Sessions and exports
//! wrap their connection in [`Logged`], which records into the [`QueryLog`] kept in memory.
//! Scripts run with a command-line client (`commands/cli.rs`) do not pass through it.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use platypus_core::{
    CancelHandle, Capabilities, CatalogColumn, CatalogForeignKey, Connection, ExecOptions,
    ExecResult, Explain, MetadataMatch, ObjectMeta, ObjectRef, Property, RecordBatchStream,
    ReferencingKey, Result, SchemaMeta, ServerInfo, TableDetails, TransactionState,
};
use serde::Serialize;

use crate::commands::lock;

/// Entries kept; older ones are dropped first.
const CAPACITY: usize = 2000;
/// Characters of a statement kept in its entry: scripts can be megabytes.
const TEXT_LIMIT: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Query,
    Metadata,
    Transaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStatus {
    Running,
    /// Rows are being read.
    Fetching,
    Done,
    /// The result was closed before its last row.
    Closed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: u64,
    /// Changes whenever the entry does, so the UI fetches only what changed.
    pub rev: u64,
    /// The saved connection's name.
    pub connection: Arc<str>,
    pub kind: LogKind,
    pub text: String,
    /// When it started, in milliseconds since the Unix epoch.
    pub started_at: u64,
    /// Until the server answered; for rows, until the last one read so far.
    pub elapsed_ms: Option<f64>,
    /// Rows read or affected.
    pub rows: Option<u64>,
    pub status: LogStatus,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LogPage {
    pub entries: Vec<LogEntry>,
    /// Pass back as `since` to get only what changes after this page.
    pub rev: u64,
}

#[derive(Debug, Default)]
pub struct QueryLog {
    entries: Mutex<VecDeque<LogEntry>>,
    next_id: AtomicU64,
    rev: AtomicU64,
}

impl QueryLog {
    fn start(&self, connection: &Arc<str>, kind: LogKind, mut text: String) -> u64 {
        if let Some((cut, _)) = text.char_indices().nth(TEXT_LIMIT) {
            text.truncate(cut);
            text.push('…');
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let mut entries = lock(&self.entries);
        if entries.len() == CAPACITY {
            entries.pop_front();
        }
        entries.push_back(LogEntry {
            id,
            rev: self.rev.fetch_add(1, Ordering::Relaxed) + 1,
            connection: Arc::clone(connection),
            kind,
            text,
            started_at,
            elapsed_ms: None,
            rows: None,
            status: LogStatus::Running,
            error: None,
        });
        id
    }

    fn update(&self, id: u64, f: impl FnOnce(&mut LogEntry)) {
        let mut entries = lock(&self.entries);
        // Ids are consecutive, so an entry's place is its distance from the oldest one.
        let Some(first) = entries.front().map(|e| e.id) else {
            return;
        };
        let Some(entry) = usize::try_from(id.wrapping_sub(first))
            .ok()
            .and_then(|i| entries.get_mut(i))
        else {
            return;
        };
        f(entry);
        entry.rev = self.rev.fetch_add(1, Ordering::Relaxed) + 1;
    }

    /// Done, or failed with `error`.
    fn finish(&self, id: u64, started: Instant, error: Option<String>) {
        self.update(id, |e| {
            e.elapsed_ms = Some(millis_since(started));
            e.status = if error.is_some() {
                LogStatus::Failed
            } else {
                LogStatus::Done
            };
            e.error = error;
        });
    }

    /// The entries that changed after `since`, oldest first.
    pub fn page(&self, since: u64) -> LogPage {
        let entries = lock(&self.entries);
        LogPage {
            entries: entries.iter().filter(|e| e.rev > since).cloned().collect(),
            rev: self.rev.load(Ordering::Relaxed),
        }
    }

    pub fn clear(&self) {
        lock(&self.entries).clear();
    }
}

fn millis_since(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

/// A connection whose requests go to the log.
#[derive(Debug)]
pub struct Logged {
    inner: Box<dyn Connection>,
    log: Arc<QueryLog>,
    connection: Arc<str>,
}

impl Logged {
    pub fn wrap(
        inner: Box<dyn Connection>,
        log: &Arc<QueryLog>,
        connection: &str,
    ) -> Arc<dyn Connection> {
        Arc::new(Self {
            inner,
            log: Arc::clone(log),
            connection: Arc::from(connection),
        })
    }

    /// Runs a request that answers all at once.
    async fn track<T>(
        &self,
        kind: LogKind,
        text: impl Into<String>,
        request: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let id = self.log.start(&self.connection, kind, text.into());
        let started = Instant::now();
        let result = request.await;
        let error = result.as_ref().err().map(ToString::to_string);
        self.log.finish(id, started, error);
        result
    }
}

#[async_trait]
impl Connection for Logged {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let id = self
            .log
            .start(&self.connection, LogKind::Query, sql.to_owned());
        let started = Instant::now();
        match self.inner.execute(sql, options).await {
            Ok(ExecResult::Affected { rows }) => {
                self.log.update(id, |e| e.rows = Some(rows));
                self.log.finish(id, started, None);
                Ok(ExecResult::Affected { rows })
            }
            Ok(ExecResult::Rows(stream)) => {
                self.log.update(id, |e| {
                    e.status = LogStatus::Fetching;
                    e.rows = Some(0);
                });
                let schema = stream.schema();
                let counted = Counted {
                    inner: stream,
                    log: Arc::clone(&self.log),
                    id,
                    started,
                    rows: 0,
                    over: false,
                };
                Ok(ExecResult::Rows(RecordBatchStream::new(
                    schema,
                    counted.boxed(),
                )))
            }
            Err(e) => {
                self.log.finish(id, started, Some(e.to_string()));
                Err(e)
            }
        }
    }

    fn cancel_handle(&self) -> CancelHandle {
        self.inner.cancel_handle()
    }

    fn server_info(&self) -> &ServerInfo {
        self.inner.server_info()
    }

    fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    async fn schemas(&self) -> Result<Vec<SchemaMeta>> {
        self.track(LogKind::Metadata, "schemas", self.inner.schemas())
            .await
    }

    async fn objects(&self, schema: &str) -> Result<Vec<ObjectMeta>> {
        let text = format!("objects of {schema}");
        self.track(LogKind::Metadata, text, self.inner.objects(schema))
            .await
    }

    async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails> {
        let text = format!("table details of {schema}.{table}");
        self.track(
            LogKind::Metadata,
            text,
            self.inner.table_details(schema, table),
        )
        .await
    }

    async fn referencing_keys(&self, schema: &str, table: &str) -> Result<Vec<ReferencingKey>> {
        let text = format!("foreign keys to {schema}.{table}");
        self.track(
            LogKind::Metadata,
            text,
            self.inner.referencing_keys(schema, table),
        )
        .await
    }

    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>> {
        self.track(
            LogKind::Metadata,
            "catalog columns",
            self.inner.catalog_columns(),
        )
        .await
    }

    async fn catalog_foreign_keys(&self) -> Result<Vec<CatalogForeignKey>> {
        self.track(
            LogKind::Metadata,
            "catalog foreign keys",
            self.inner.catalog_foreign_keys(),
        )
        .await
    }

    fn transaction(&self) -> TransactionState {
        self.inner.transaction()
    }

    async fn set_autocommit(&self, on: bool) -> Result<()> {
        let text = format!("autocommit {}", if on { "on" } else { "off" });
        self.track(LogKind::Transaction, text, self.inner.set_autocommit(on))
            .await
    }

    async fn commit(&self) -> Result<()> {
        self.track(LogKind::Transaction, "commit", self.inner.commit())
            .await
    }

    async fn rollback(&self) -> Result<()> {
        self.track(LogKind::Transaction, "rollback", self.inner.rollback())
            .await
    }

    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    async fn explain(&self, sql: &str, analyze: bool) -> Result<Explain> {
        let text = format!("EXPLAIN{} {sql}", if analyze { " ANALYZE" } else { "" });
        self.track(LogKind::Query, text, self.inner.explain(sql, analyze))
            .await
    }

    async fn object_ddl(&self, object: &ObjectRef<'_>) -> Result<String> {
        let text = format!("DDL of {}.{}", object.schema, object.name);
        self.track(LogKind::Metadata, text, self.inner.object_ddl(object))
            .await
    }

    async fn object_properties(&self, object: &ObjectRef<'_>) -> Result<Vec<Property>> {
        let text = format!("properties of {}.{}", object.schema, object.name);
        self.track(
            LogKind::Metadata,
            text,
            self.inner.object_properties(object),
        )
        .await
    }

    async fn databases(&self) -> Result<Vec<String>> {
        self.track(LogKind::Metadata, "databases", self.inner.databases())
            .await
    }

    async fn search_metadata(&self, needle: &str) -> Result<Vec<MetadataMatch>> {
        let text = format!("search for {needle}");
        self.track(LogKind::Metadata, text, self.inner.search_metadata(needle))
            .await
    }
}

/// Rows passing through to the reader, counted into the log entry batch by batch.
struct Counted {
    inner: RecordBatchStream,
    log: Arc<QueryLog>,
    id: u64,
    started: Instant,
    rows: u64,
    over: bool,
}

impl Stream for Counted {
    type Item = Result<arrow_array::RecordBatch>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next = self.inner.poll_next_unpin(cx);
        let this = &mut *self;
        match &next {
            Poll::Ready(Some(Ok(batch))) => {
                this.rows += batch.num_rows() as u64;
                let (rows, started) = (this.rows, this.started);
                this.log.update(this.id, |e| {
                    e.rows = Some(rows);
                    e.elapsed_ms = Some(millis_since(started));
                });
            }
            Poll::Ready(Some(Err(e))) => {
                this.over = true;
                this.log.finish(this.id, this.started, Some(e.to_string()));
            }
            Poll::Ready(None) => {
                this.over = true;
                this.log.finish(this.id, this.started, None);
            }
            Poll::Pending => {}
        }
        next
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        if !self.over {
            self.log.update(self.id, |e| e.status = LogStatus::Closed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_only_what_changed_and_drops_the_oldest() {
        let log = QueryLog::default();
        let conn: Arc<str> = Arc::from("db");
        let first = log.start(&conn, LogKind::Query, "SELECT 1".to_owned());
        let page = log.page(0);
        assert_eq!(page.entries.len(), 1);
        assert!(log.page(page.rev).entries.is_empty());

        log.finish(first, Instant::now(), None);
        let changed = log.page(page.rev).entries;
        assert_eq!((changed.len(), changed[0].status), (1, LogStatus::Done));

        for _ in 0..CAPACITY {
            log.start(&conn, LogKind::Metadata, "schemas".to_owned());
        }
        let all = log.page(0).entries;
        assert_eq!(all.len(), CAPACITY);
        assert_ne!(all[0].id, first);
        // An entry no longer kept is ignored.
        log.finish(first, Instant::now(), Some("gone".to_owned()));
        assert!(
            log.page(0)
                .entries
                .iter()
                .all(|e| e.status != LogStatus::Failed)
        );
    }

    #[test]
    fn logs_what_a_connection_runs() {
        use futures::TryStreamExt;
        use platypus_core::{ConnectionConfig, Driver, DriverKind};

        tauri::async_runtime::block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("log.db");
            std::fs::File::create(&path).unwrap();
            let mut config = ConnectionConfig::new(DriverKind::Sqlite);
            config.database = path.to_string_lossy().into_owned();
            let log = Arc::new(QueryLog::default());
            let conn = platypus_driver_sqlite::SqliteDriver
                .connect(&config)
                .await
                .unwrap();
            let conn = Logged::wrap(conn, &log, "local");
            let options = ExecOptions { batch_size: 2 };

            conn.execute("CREATE TABLE t (id INT)", &options)
                .await
                .unwrap();
            conn.execute("INSERT INTO t VALUES (1), (2), (3)", &options)
                .await
                .unwrap();
            let ExecResult::Rows(stream) =
                conn.execute("SELECT id FROM t", &options).await.unwrap()
            else {
                panic!("rows expected");
            };
            let batches: Vec<_> = stream.try_collect().await.unwrap();
            assert_eq!(batches.len(), 2);
            let ExecResult::Rows(mut stream) =
                conn.execute("SELECT id FROM t", &options).await.unwrap()
            else {
                panic!("rows expected");
            };
            stream.next().await.unwrap().unwrap();
            drop(stream);
            conn.execute("SELECT nope", &options).await.unwrap_err();
            conn.schemas().await.unwrap();

            let entries = log.page(0).entries;
            let seen: Vec<_> = entries.iter().map(|e| (e.kind, e.status, e.rows)).collect();
            assert_eq!(
                seen,
                [
                    (LogKind::Query, LogStatus::Done, Some(0)),
                    (LogKind::Query, LogStatus::Done, Some(3)),
                    (LogKind::Query, LogStatus::Done, Some(3)),
                    (LogKind::Query, LogStatus::Closed, Some(2)),
                    (LogKind::Query, LogStatus::Failed, None),
                    (LogKind::Metadata, LogStatus::Done, None),
                ]
            );
            assert!(
                entries[4]
                    .error
                    .as_deref()
                    .is_some_and(|e| e.contains("nope"))
            );
            assert!(
                entries
                    .iter()
                    .all(|e| &*e.connection == "local" && e.elapsed_ms.is_some())
            );
        });
    }

    #[test]
    fn cuts_long_statements() {
        let log = QueryLog::default();
        log.start(&Arc::from("db"), LogKind::Query, "é".repeat(TEXT_LIMIT + 5));
        let text = &log.page(0).entries[0].text;
        assert_eq!(text.chars().count(), TEXT_LIMIT + 1);
        assert!(text.ends_with('…'));
    }
}
