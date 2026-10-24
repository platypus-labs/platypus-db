//! SQLite driver built on `rusqlite`.
//!
//! Each connection owns a worker thread that holds the `rusqlite::Connection` and runs
//! statements one at a time. Rows travel to the async side through a one-slot channel, so
//! the worker only steps the statement as fast as batches are consumed. Cancellation uses
//! `sqlite3_interrupt`.

#[doc(hidden)]
pub mod columns;
mod meta;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc as std_mpsc};
use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_schema::{Schema, SchemaRef};
use async_trait::async_trait;
use futures::StreamExt;
use platypus_core::{
    CancelHandle, Canceller, CatalogColumn, Connection, ConnectionConfig, DbError, Driver,
    DriverKind, Error, ExecOptions, ExecResult, ObjectMeta, RecordBatchStream, ReferencingKey,
    Result, SchemaMeta, ServerInfo, TableDetails, TransactionState, TxEffect, classify,
    open_transaction_error,
};
use rusqlite::types::ValueRef;
use rusqlite::{ErrorCode, InterruptHandle, OpenFlags};
use tokio::sync::{mpsc, oneshot};

use crate::columns::{ColumnBuilder, Kind, Mismatch, Owned};

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Default, Clone, Copy)]
pub struct SqliteDriver;

#[async_trait]
impl Driver for SqliteDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Sqlite
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        Ok(Box::new(SqliteConnection::connect(config).await?))
    }
}

#[derive(Debug)]
pub struct SqliteConnection {
    commands: std_mpsc::Sender<Command>,
    canceller: SqliteCanceller,
    server: ServerInfo,
    next_job: AtomicU64,
    autocommit: AtomicBool,
    /// `!sqlite3_get_autocommit()`, published by the worker after each statement.
    in_transaction: Arc<AtomicBool>,
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

fn open(config: &ConnectionConfig) -> Result<rusqlite::Connection> {
    let path = config.database.as_str();
    if path.is_empty() {
        return Err(Error::Config("no database file given".to_owned()));
    }
    let memory = path == ":memory:";
    if !memory && !path.starts_with("file:") && !Path::new(path).exists() {
        return Err(Error::Connect(format!("{path} does not exist")));
    }
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if memory {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let conn = rusqlite::Connection::open_with_flags(path, flags)
        .map_err(|e| Error::Connect(format!("cannot open {path}: {e}")))?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| map_error(e, ""))?;
    Ok(conn)
}

impl SqliteConnection {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self> {
        if config.driver != DriverKind::Sqlite {
            return Err(Error::Config(format!(
                "{:?} is not a SQLite config",
                config.driver
            )));
        }
        let config = config.clone();
        let (commands, inbox) = std_mpsc::channel();
        let (ready, opened) = oneshot::channel();
        let running = Arc::new(Mutex::new(None));
        let worker_running = Arc::clone(&running);
        let in_transaction = Arc::new(AtomicBool::new(false));
        let worker_in_transaction = Arc::clone(&in_transaction);
        std::thread::Builder::new()
            .name("platypus-sqlite".to_owned())
            .spawn(move || match open(&config) {
                Ok(conn) => {
                    let _ = ready.send(Ok(conn.get_interrupt_handle()));
                    worker(&conn, &inbox, &worker_running, &worker_in_transaction);
                }
                Err(e) => {
                    let _ = ready.send(Err(e));
                }
            })
            .map_err(|e| Error::Connect(format!("cannot start the SQLite worker: {e}")))?;
        let interrupt = opened.await.map_err(|_| Error::Closed)??;

        Ok(Self {
            commands,
            canceller: SqliteCanceller {
                interrupt: Arc::new(interrupt),
                running,
            },
            server: ServerInfo {
                product: "SQLite".to_owned(),
                version: rusqlite::version().to_owned(),
                encrypted: false,
            },
            next_job: AtomicU64::new(1),
            autocommit: AtomicBool::new(true),
            in_transaction,
        })
    }
}

#[async_trait]
impl Connection for SqliteConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let state = self.transaction();
        if !state.autocommit && !state.open && classify(sql) == TxEffect::None {
            self.send("BEGIN", 1).await?;
        }
        let (job, started) = self.send(sql, options.batch_size.max(1)).await?;
        match started {
            Started::Affected(rows) => Ok(ExecResult::Affected { rows }),
            Started::Rows(schema, batches) => {
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

    async fn catalog_foreign_keys(&self) -> Result<Vec<platypus_core::CatalogForeignKey>> {
        meta::catalog_foreign_keys(self).await
    }

    async fn object_ddl(&self, object: &platypus_core::ObjectRef<'_>) -> Result<String> {
        meta::object_ddl(self, object).await
    }

    fn transaction(&self) -> TransactionState {
        TransactionState {
            autocommit: self.autocommit.load(Ordering::Acquire),
            open: self.in_transaction.load(Ordering::Acquire),
        }
    }

    async fn set_autocommit(&self, on: bool) -> Result<()> {
        if self.transaction().open {
            return Err(open_transaction_error());
        }
        self.autocommit.store(on, Ordering::Release);
        Ok(())
    }

    async fn commit(&self) -> Result<()> {
        if self.transaction().open {
            self.send("COMMIT", 1).await?;
        }
        Ok(())
    }

    async fn rollback(&self) -> Result<()> {
        if self.transaction().open {
            self.send("ROLLBACK", 1).await?;
        }
        Ok(())
    }

    fn capabilities(&self) -> platypus_core::Capabilities {
        platypus_core::Capabilities {
            transactions: true,
            cancel: true,
            object_ddl: true,
            ..Default::default()
        }
    }
}

impl SqliteConnection {
    /// Queues a statement on the worker and waits until it starts returning.
    async fn send(&self, sql: &str, batch_size: usize) -> Result<(u64, Started)> {
        let job = self.next_job.fetch_add(1, Ordering::Relaxed);
        let (reply, started) = oneshot::channel();
        self.commands
            .send(Command {
                job,
                sql: sql.to_owned(),
                batch_size,
                reply,
            })
            .map_err(|_| Error::Closed)?;
        Ok((job, started.await.map_err(|_| Error::Closed)??))
    }
}

/// The receiving end of a statement's rows. Dropping it early interrupts the statement,
/// unless the worker has already moved on to another one.
struct Batches {
    rx: mpsc::Receiver<Result<RecordBatch>>,
    job: u64,
    canceller: SqliteCanceller,
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
        if !self.done {
            self.canceller.interrupt_job(Some(self.job));
        }
    }
}

#[derive(Clone)]
struct SqliteCanceller {
    interrupt: Arc<InterruptHandle>,
    /// The job the worker is running. Locked while interrupting, so the worker cannot
    /// switch to the next statement in between.
    running: Arc<Mutex<Option<u64>>>,
}

impl std::fmt::Debug for SqliteCanceller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteCanceller").finish_non_exhaustive()
    }
}

impl SqliteCanceller {
    /// Interrupts `job`, or whatever is running when `None`.
    fn interrupt_job(&self, job: Option<u64>) {
        let running = lock(&self.running);
        if running.is_some() && (job.is_none() || *running == job) {
            self.interrupt.interrupt();
        }
    }
}

#[async_trait]
impl Canceller for SqliteCanceller {
    async fn cancel(&self) -> Result<()> {
        self.interrupt_job(None);
        Ok(())
    }
}

fn worker(
    conn: &rusqlite::Connection,
    inbox: &std_mpsc::Receiver<Command>,
    running: &Mutex<Option<u64>>,
    in_transaction: &AtomicBool,
) {
    // Ends when the connection, and with it the sender, is dropped.
    for command in inbox {
        *lock(running) = Some(command.job);
        run(conn, command, in_transaction);
        *lock(running) = None;
    }
}

fn run(conn: &rusqlite::Connection, command: Command, in_transaction: &AtomicBool) {
    let Command {
        sql,
        batch_size,
        reply,
        ..
    } = command;
    let mut stmt = match conn.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(e) => {
            let _ = reply.send(Err(map_error(e, &sql)));
            return;
        }
    };

    if stmt.column_count() == 0 {
        let before = conn.total_changes();
        let result = stmt.raw_execute().map_err(|e| map_error(e, &sql));
        // `changes()` keeps the last DML's count across DDL statements.
        let changed = conn.total_changes() != before;
        in_transaction.store(!conn.is_autocommit(), Ordering::Release);
        let _ =
            reply.send(result.map(|_| Started::Affected(if changed { conn.changes() } else { 0 })));
        return;
    }

    stream_rows(&mut stmt, &sql, batch_size, reply);
}

fn value<'a>(row: &'a rusqlite::Row<'_>, i: usize) -> ValueRef<'a> {
    row.get_ref(i).unwrap_or(ValueRef::Null)
}

/// Answers with the schema, then streams the rows of a statement that returns some.
fn stream_rows(
    stmt: &mut rusqlite::Statement<'_>,
    sql: &str,
    batch_size: usize,
    reply: oneshot::Sender<Result<Started>>,
) {
    let columns: Vec<(String, Option<String>)> = stmt
        .columns()
        .iter()
        .map(|c| (c.name().to_owned(), c.decl_type().map(str::to_owned)))
        .collect();
    let width = columns.len();
    let mut rows = stmt.raw_query();

    // The first batch is read before answering, both to infer column types and so that
    // errors raised by the first rows fail the statement itself.
    let mut first: Vec<Vec<Owned>> = Vec::new();
    let mut exhausted = false;
    while first.len() < batch_size {
        match rows.next() {
            Ok(Some(row)) => first.push((0..width).map(|i| Owned::new(value(row, i))).collect()),
            Ok(None) => {
                exhausted = true;
                break;
            }
            Err(e) => {
                let _ = reply.send(Err(map_error(e, sql)));
                return;
            }
        }
    }

    let kinds: Vec<Kind> = columns
        .iter()
        .enumerate()
        .map(|(i, (_, decl))| columns::infer(decl.as_deref(), first.iter().map(|r| r[i].value())))
        .collect();
    let schema: SchemaRef = Arc::new(Schema::new(
        columns
            .iter()
            .zip(&kinds)
            .map(|((name, decl), kind)| columns::field(name, decl.as_deref(), *kind))
            .collect::<Vec<_>>(),
    ));
    let (tx, rx) = mpsc::channel(1);
    if reply
        .send(Ok(Started::Rows(Arc::clone(&schema), rx)))
        .is_err()
    {
        return;
    }

    let mut out = BatchOut::new(schema, &kinds, batch_size, tx);
    if !first.is_empty() {
        let appended = first
            .iter()
            .try_for_each(|row| out.append(row.iter().map(Owned::value)));
        if !out.flush(appended) {
            return;
        }
    }
    drop(first);
    if exhausted {
        return;
    }
    loop {
        let mut len = 0;
        let mut end = false;
        while len < batch_size {
            match rows.next() {
                Ok(Some(row)) => {
                    if let Err(e) = out.append((0..width).map(|i| value(row, i))) {
                        out.flush(Err(e));
                        return;
                    }
                    len += 1;
                }
                Ok(None) => {
                    end = true;
                    break;
                }
                Err(e) => {
                    out.flush(Err(map_error(e, sql)));
                    return;
                }
            }
        }
        if (len > 0 && !out.flush(Ok(()))) || end {
            return;
        }
    }
}

/// Builds batches on the worker and hands them to the consumer.
struct BatchOut {
    schema: SchemaRef,
    builders: Vec<ColumnBuilder>,
    tx: mpsc::Sender<Result<RecordBatch>>,
}

impl BatchOut {
    fn new(
        schema: SchemaRef,
        kinds: &[Kind],
        batch_size: usize,
        tx: mpsc::Sender<Result<RecordBatch>>,
    ) -> Self {
        Self {
            schema,
            builders: kinds
                .iter()
                .map(|k| ColumnBuilder::new(*k, batch_size))
                .collect(),
            tx,
        }
    }

    fn append<'a>(&mut self, row: impl Iterator<Item = ValueRef<'a>>) -> Result<()> {
        for (i, value) in row.enumerate() {
            if let Err(m) = self.builders[i].append(value) {
                return Err(self.mismatch(i, &m));
            }
        }
        Ok(())
    }

    fn mismatch(&self, column: usize, m: &Mismatch) -> Error {
        let field = self.schema.field(column);
        Error::Decode {
            column: field.name().clone(),
            db_type: field
                .metadata()
                .get(platypus_core::META_DB_TYPE)
                .cloned()
                .unwrap_or_default(),
            message: format!(
                "a {} value follows {} values in the first rows; use CAST(... AS TEXT) \
                 for columns with mixed types",
                m.found, m.expected
            ),
        }
    }

    /// Sends the rows appended so far, or `status` when it is an error, waiting for the
    /// consumer to make room. Returns `false` when the statement should stop.
    fn flush(&mut self, status: Result<()>) -> bool {
        let batch = status.and_then(|()| {
            let arrays = self
                .builders
                .iter_mut()
                .map(ColumnBuilder::finish)
                .collect();
            Ok(RecordBatch::try_new(Arc::clone(&self.schema), arrays)?)
        });
        let ok = batch.is_ok();
        self.tx.blocking_send(batch).is_ok() && ok
    }
}

fn map_error(e: rusqlite::Error, sql: &str) -> Error {
    match e {
        rusqlite::Error::SqliteFailure(f, _) if f.code == ErrorCode::OperationInterrupted => {
            Error::Cancelled
        }
        rusqlite::Error::SqliteFailure(f, message) => Error::Database(DbError {
            code: Some(f.extended_code.to_string()),
            message: message.unwrap_or_else(|| f.to_string()),
            ..DbError::default()
        }),
        rusqlite::Error::SqlInputError {
            error, msg, offset, ..
        } => Error::Database(DbError {
            code: Some(error.extended_code.to_string()),
            message: msg,
            position: usize::try_from(offset)
                .ok()
                .and_then(|o| sql.get(..o))
                .and_then(|prefix| u32::try_from(prefix.chars().count() + 1).ok()),
            ..DbError::default()
        }),
        other => Error::Other(other.to_string()),
    }
}
