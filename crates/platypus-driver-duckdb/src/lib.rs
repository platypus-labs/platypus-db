//! DuckDB driver built on the `duckdb` crate, with DuckDB's library downloaded at first use
//! ([`library`]).
//!
//! DuckDB runs in-process, like SQLite: each connection owns a worker thread that holds the
//! `duckdb::Connection` and runs statements one at a time. DuckDB produces Arrow natively,
//! so the worker forwards the record batches of its streaming results as they are, only
//! sliced to the requested batch size and tagged with the DuckDB type names. Batches travel
//! through a one-slot channel, so the worker only fetches as fast as they are consumed.
//! Cancellation uses `duckdb_interrupt`.

mod bulk;
mod csvfile;
mod database;
mod error;
mod explain;
pub mod files;
pub mod library;
mod meta;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod types;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc as std_mpsc};

use arrow_array::{Array, RecordBatch, StructArray};
use arrow_schema::{DataType, Schema, SchemaRef};
use async_trait::async_trait;
use duckdb::InterruptHandle;
use futures::StreamExt;
use platypus_core::{
    CancelHandle, Canceller, CatalogColumn, CatalogForeignKey, Connection, ConnectionConfig,
    Driver, DriverKind, Error, ExecOptions, ExecResult, MetadataMatch, ObjectMeta, ObjectRef,
    Property, RecordBatchStream, ReferencingKey, Result, SchemaMeta, ServerInfo, TableDetails,
    TransactionState, TxTracker, classify,
};
use platypus_sql::{Dialect, Lexer, TokenKind};
use tokio::sync::{mpsc, oneshot};

use crate::error::map_error;

#[derive(Debug, Default, Clone, Copy)]
pub struct DuckDbDriver;

#[async_trait]
impl Driver for DuckDbDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::DuckDb
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let conn: Box<dyn Connection> = Box::new(DuckDbConnection::connect(config).await?);
        platypus_core::run_init(&*conn, &config.init, self.kind()).await?;
        Ok(conn)
    }
}

/// An in-memory database of its own, kept while this handle or a connection to it lives.
/// Sessions reach it at [`memory_path`]; `:memory:` alone is the one the app's sessions share.
pub struct MemoryDatabase {
    _database: Arc<database::Database>,
}

impl std::fmt::Debug for MemoryDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryDatabase").finish_non_exhaustive()
    }
}

/// Where sessions connect to the in-memory database `name`.
#[must_use]
pub fn memory_path(name: &str) -> String {
    format!(":memory:{name}")
}

/// Opens the in-memory database `name`, or finds it open, with a connection to fill it.
pub fn memory_database(name: &str) -> Result<(MemoryDatabase, duckdb::Connection)> {
    let (database, conn) = database::connect(&memory_path(name), &files::ReadOptions::default())?;
    Ok((
        MemoryDatabase {
            _database: database,
        },
        conn,
    ))
}

#[derive(Debug)]
pub struct DuckDbConnection {
    commands: std_mpsc::Sender<Job>,
    canceller: DuckDbCanceller,
    server: ServerInfo,
    next_job: AtomicU64,
    tx: Arc<TxTracker>,
    /// Held from the implicit `BEGIN` of manual-commit mode until the statement is queued,
    /// so that two statements sent at once do not both begin.
    submit: tokio::sync::Mutex<()>,
}

/// What the worker runs, one at a time: a statement, or rows to append.
enum Job {
    Statement(Command),
    Load {
        job: u64,
        load: bulk::Load,
        /// No transaction is open: the load runs in one of its own.
        own_transaction: bool,
        reply: oneshot::Sender<Result<u64>>,
    },
}

struct Command {
    job: u64,
    sql: String,
    batch_size: usize,
    reply: oneshot::Sender<Result<Started>>,
}

enum Started {
    Affected(u64),
    Rows(SchemaRef, mpsc::Receiver<Result<RecordBatch>>),
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl DuckDbConnection {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self> {
        if config.driver != DriverKind::DuckDb {
            return Err(Error::Config(format!(
                "{:?} is not a DuckDB config",
                config.driver
            )));
        }
        let config = config.clone();
        let (commands, inbox) = std_mpsc::channel();
        let (ready, opened) = oneshot::channel();
        let running = Arc::new(Mutex::new(None));
        let worker_running = Arc::clone(&running);
        let tx = Arc::new(TxTracker::default());
        std::thread::Builder::new()
            .name("platypus-duckdb".to_owned())
            .spawn(move || {
                match database::connect(&config.database, &files::ReadOptions::of(&config.options))
                {
                    // The connection is dropped before its database.
                    Ok((_database, conn)) => {
                        let version = conn.version().unwrap_or_default();
                        let _ = ready.send(Ok((conn.interrupt_handle(), version)));
                        worker(&conn, &inbox, &worker_running);
                    }
                    Err(e) => {
                        let _ = ready.send(Err(e));
                    }
                }
            })
            .map_err(|e| Error::Connect(format!("cannot start the DuckDB worker: {e}")))?;
        let (interrupt, version) = opened.await.map_err(|_| Error::Closed)??;

        Ok(Self {
            commands,
            canceller: DuckDbCanceller {
                interrupt,
                running,
                tx: Arc::clone(&tx),
            },
            server: ServerInfo {
                product: "DuckDB".to_owned(),
                version: version.trim_start_matches('v').to_owned(),
                encrypted: false,
            },
            next_job: AtomicU64::new(1),
            tx,
            submit: tokio::sync::Mutex::new(()),
        })
    }

    /// Queues a statement on the worker and waits until it starts returning.
    async fn send(&self, sql: &str, batch_size: usize) -> Result<(u64, Started)> {
        let job = self.next_job.fetch_add(1, Ordering::Relaxed);
        let (reply, started) = oneshot::channel();
        self.commands
            .send(Job::Statement(Command {
                job,
                sql: sql.to_owned(),
                batch_size,
                reply,
            }))
            .map_err(|_| Error::Closed)?;
        Ok((job, started.await.map_err(|_| Error::Closed)??))
    }

    /// Ends the open transaction, if any. A failed `COMMIT` rolls back in DuckDB, so the
    /// transaction is over either way.
    async fn end_transaction(&self, sql: &str) -> Result<()> {
        let _guard = self.submit.lock().await;
        if !self.tx.state().open {
            return Ok(());
        }
        let result = self.send(sql, 1).await.map(|_| ());
        self.tx.set_open(false);
        result
    }
}

#[async_trait]
impl Connection for DuckDbConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let effect = classify(sql);
        let guard = self.submit.lock().await;
        if self.tx.needs_begin(effect) {
            self.send("BEGIN", 1).await?;
            self.tx.set_open(true);
        }
        let started = self.send(sql, options.batch_size.max(1)).await;
        // DDL is transactional in DuckDB; a failed statement leaves the transaction open
        // (aborted) until ROLLBACK.
        self.tx.after(effect, false);
        drop(guard);
        match started? {
            (_, Started::Affected(rows)) => Ok(ExecResult::Affected { rows }),
            (job, Started::Rows(schema, batches)) => {
                let batches = Batches {
                    rx: batches,
                    job,
                    canceller: self.canceller.clone(),
                    done: false,
                };
                let stream = futures::stream::unfold(batches, |mut b| async move {
                    b.next().await.map(|item| (item, b))
                });
                Ok(ExecResult::Rows(RecordBatchStream::new(
                    schema,
                    stream.boxed(),
                )))
            }
        }
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(self.canceller.clone())
    }

    fn server_info(&self) -> &ServerInfo {
        &self.server
    }

    /// An in-process database cannot be lost.
    fn is_closed(&self) -> bool {
        false
    }

    async fn schemas(&self) -> Result<Vec<SchemaMeta>> {
        meta::schemas(self).await
    }

    async fn objects(&self, schema: &str) -> Result<Vec<ObjectMeta>> {
        meta::objects(self, schema).await
    }

    async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails> {
        meta::table_details(self, schema, table).await
    }

    async fn referencing_keys(&self, schema: &str, table: &str) -> Result<Vec<ReferencingKey>> {
        meta::referencing_keys(self, schema, table).await
    }

    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>> {
        meta::catalog_columns(self).await
    }

    async fn catalog_foreign_keys(&self) -> Result<Vec<CatalogForeignKey>> {
        meta::catalog_foreign_keys(self).await
    }

    async fn object_ddl(&self, object: &ObjectRef<'_>) -> Result<String> {
        meta::object_ddl(self, object).await
    }

    async fn explain(&self, sql: &str, analyze: bool) -> Result<platypus_core::Explain> {
        explain::explain(self, sql, analyze).await
    }

    async fn object_properties(&self, object: &ObjectRef<'_>) -> Result<Vec<Property>> {
        meta::object_properties(self, object).await
    }

    async fn search_metadata(&self, needle: &str) -> Result<Vec<MetadataMatch>> {
        meta::search_metadata(self, needle).await
    }

    fn transaction(&self) -> TransactionState {
        self.tx.state()
    }

    async fn set_autocommit(&self, on: bool) -> Result<()> {
        self.tx.ensure_closed()?;
        self.tx.set_autocommit(on);
        Ok(())
    }

    async fn commit(&self) -> Result<()> {
        self.end_transaction("COMMIT").await
    }

    async fn rollback(&self) -> Result<()> {
        self.end_transaction("ROLLBACK").await
    }

    fn capabilities(&self) -> platypus_core::Capabilities {
        platypus_core::Capabilities {
            transactions: true,
            cancel: true,
            explain: true,
            object_ddl: true,
            metadata_search: true,
            bulk_load: true,
            ..Default::default()
        }
    }

    /// The rows go in with the appender, in the session's transaction (one opened in
    /// manual-commit mode, as a statement would), else in one of their own.
    async fn bulk_load(&self, rows: &platypus_core::BulkRows<'_>) -> Result<u64> {
        let load = bulk::Load::new(rows)?;
        let _guard = self.submit.lock().await;
        if self.tx.needs_begin(platypus_core::TxEffect::None) {
            self.send("BEGIN", 1).await?;
            self.tx.set_open(true);
        }
        let job = self.next_job.fetch_add(1, Ordering::Relaxed);
        let (reply, written) = oneshot::channel();
        let own_transaction = !self.tx.state().open;
        self.commands
            .send(Job::Load {
                job,
                load,
                own_transaction,
                reply,
            })
            .map_err(|_| Error::Closed)?;
        written.await.map_err(|_| Error::Closed)?
    }
}

/// The receiving end of a statement's rows. Dropping it early stops the statement.
struct Batches {
    rx: mpsc::Receiver<Result<RecordBatch>>,
    job: u64,
    canceller: DuckDbCanceller,
    done: bool,
}

impl Batches {
    async fn next(&mut self) -> Option<Result<RecordBatch>> {
        let item = self.rx.recv().await;
        self.done = item.is_none();
        item
    }
}

impl Drop for Batches {
    fn drop(&mut self) {
        // The worker notices the closed channel at its next batch. Interrupting reaches it
        // sooner, but an interrupted statement aborts the transaction it runs in.
        if !self.done && !self.canceller.tx.state().open {
            self.canceller.interrupt_job(Some(self.job));
        }
    }
}

#[derive(Clone)]
struct DuckDbCanceller {
    interrupt: Arc<InterruptHandle>,
    /// The job the worker is running. Locked while interrupting, so the worker cannot
    /// switch to the next statement in between.
    running: Arc<Mutex<Option<u64>>>,
    tx: Arc<TxTracker>,
}

impl std::fmt::Debug for DuckDbCanceller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DuckDbCanceller").finish_non_exhaustive()
    }
}

impl DuckDbCanceller {
    /// Interrupts `job`, or whatever is running when `None`.
    fn interrupt_job(&self, job: Option<u64>) {
        let running = lock(&self.running);
        if running.is_some() && (job.is_none() || *running == job) {
            self.interrupt.interrupt();
        }
    }
}

#[async_trait]
impl Canceller for DuckDbCanceller {
    async fn cancel(&self) -> Result<()> {
        self.interrupt_job(None);
        Ok(())
    }
}

fn worker(
    conn: &duckdb::Connection,
    inbox: &std_mpsc::Receiver<Job>,
    running: &Mutex<Option<u64>>,
) {
    // Ends when the connection, and with it the sender, is dropped.
    for job in inbox {
        match job {
            Job::Statement(command) => {
                *lock(running) = Some(command.job);
                run(conn, command);
            }
            Job::Load {
                job,
                load,
                own_transaction,
                reply,
            } => {
                *lock(running) = Some(job);
                let _ = reply.send(bulk::append(conn, load, own_transaction));
            }
        }
        *lock(running) = None;
    }
}

/// Leading words of statements that always answer with rows, even when their result looks
/// like a row count.
const QUERIES: &[&str] = &[
    "SELECT",
    "WITH",
    "FROM",
    "VALUES",
    "TABLE",
    "SHOW",
    "DESCRIBE",
    "DESC",
    "SUMMARIZE",
    "PRAGMA",
    "CALL",
    "EXPLAIN",
    "PIVOT",
    "UNPIVOT",
];

/// What a statement answers when DuckDB's result is not rows for the user: DML and `COPY`
/// return their row count as a single `Count` column, other DDL an empty `Count` or
/// `Success` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Rows,
    Count,
    Done,
}

fn outcome(sql: &str, schema: &Schema) -> Outcome {
    let mut words = Lexer::new(sql, Dialect::DuckDb)
        .filter(|t| t.kind == TokenKind::Word)
        .map(|t| t.text(sql).to_ascii_uppercase());
    let Some(first) = words.next() else {
        return Outcome::Rows;
    };
    if QUERIES.contains(&first.as_str()) || words.any(|w| w == "RETURNING") {
        return Outcome::Rows;
    }
    let only = match &schema.fields()[..] {
        [field] => Some((field.name().as_str(), field.data_type())),
        _ => None,
    };
    match only {
        Some(("Count", DataType::Int64)) => Outcome::Count,
        Some(("Success", DataType::Boolean)) => Outcome::Done,
        _ => Outcome::Rows,
    }
}

fn run(conn: &duckdb::Connection, command: Command) {
    let Command {
        sql,
        batch_size,
        reply,
        ..
    } = command;
    let mut stmt = match conn.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(e) => {
            let _ = reply.send(Err(map_error(&e, &sql)));
            return;
        }
    };
    let schema = match stmt.stream_arrow([]) {
        Ok(arrow) => arrow.get_schema(),
        Err(e) => {
            let _ = reply.send(Err(map_error(&e, &sql)));
            return;
        }
    };
    match outcome(&sql, &schema) {
        Outcome::Rows => {}
        kind => {
            let _ = reply.send(row_count(&stmt, kind).map_err(|e| map_error(&e, &sql)));
            return;
        }
    }
    let schema = types::tag_schema(&schema, &stmt);
    // The first chunk is fetched before answering, so errors raised by the first rows fail
    // the statement itself.
    let mut chunk = match stmt.step() {
        Ok(chunk) => chunk,
        Err(e) => {
            let _ = reply.send(Err(map_error(&e, &sql)));
            return;
        }
    };
    let (tx, rx) = mpsc::channel(1);
    if reply
        .send(Ok(Started::Rows(Arc::clone(&schema), rx)))
        .is_err()
    {
        return;
    }
    while let Some(array) = chunk {
        match conform(&schema, &array) {
            Ok(batch) => {
                if !send_sliced(&tx, &batch, batch_size) {
                    return;
                }
            }
            Err(e) => {
                let _ = tx.blocking_send(Err(e));
                return;
            }
        }
        chunk = match stmt.step() {
            Ok(next) => next,
            Err(e) => {
                let _ = tx.blocking_send(Err(map_error(&e, &sql)));
                return;
            }
        };
    }
}

/// A chunk as a batch of `schema`, its columns cast where [`types::portable`] changed
/// their type.
fn conform(schema: &SchemaRef, chunk: &StructArray) -> Result<RecordBatch> {
    let columns = chunk
        .columns()
        .iter()
        .zip(schema.fields())
        .map(|(column, field)| {
            if column.data_type() == field.data_type() {
                Ok(Arc::clone(column))
            } else if let Some(bytes) = column.as_any().downcast_ref::<arrow_array::BinaryArray>()
                && *field.data_type() == arrow_schema::DataType::Utf8
            {
                let bits: arrow_array::StringArray =
                    bytes.iter().map(|b| b.and_then(types::bit_text)).collect();
                Ok(Arc::new(bits) as arrow_array::ArrayRef)
            } else if let Some(union) = column.as_any().downcast_ref::<arrow_array::UnionArray>() {
                Ok(Arc::new(types::union_text(union)?) as arrow_array::ArrayRef)
            } else {
                arrow_cast::cast(column, field.data_type())
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RecordBatch::try_new(Arc::clone(schema), columns)?)
}

/// Sends `batch` in pieces of at most `batch_size` rows, waiting for the consumer to make
/// room. Returns `false` once the consumer is gone.
fn send_sliced(
    tx: &mpsc::Sender<Result<RecordBatch>>,
    batch: &RecordBatch,
    batch_size: usize,
) -> bool {
    let rows = batch.num_rows();
    if rows <= batch_size {
        return tx.blocking_send(Ok(batch.clone())).is_ok();
    }
    (0..rows).step_by(batch_size).all(|offset| {
        tx.blocking_send(Ok(batch.slice(offset, batch_size.min(rows - offset))))
            .is_ok()
    })
}

/// The total of a `Count` result, or 0 for an empty one.
fn row_count(stmt: &duckdb::Statement<'_>, kind: Outcome) -> duckdb::Result<Started> {
    let mut rows = 0;
    while let Some(array) = stmt.step()? {
        if kind == Outcome::Count
            && let Some(counts) = array
                .column(0)
                .as_any()
                .downcast_ref::<arrow_array::Int64Array>()
        {
            rows += counts
                .iter()
                .flatten()
                .map(|n| u64::try_from(n).unwrap_or(0))
                .sum::<u64>();
        }
    }
    Ok(Started::Affected(rows))
}

#[cfg(test)]
mod tests;
