//! Microsoft SQL Server and Azure SQL driver built on `tiberius` (vendored with patches in
//! `vendor/tiberius`).
//!
//! A statement runs as a SQL batch on a task that owns the session for its whole lifetime
//! and sends batches of rows through a one-slot channel, so rows are read from the socket
//! only as fast as they are consumed. Cancelling, or dropping an unfinished stream, sends a
//! TDS attention: the server stops the batch and the session stays usable, with its
//! transaction, temporary tables and settings.
//!
//! A batch can return several result sets; like the other drivers, `execute` streams the
//! first one and reads the rest to the end without returning them, so every statement of
//! the batch runs and its errors are reported. A batch without results reports the rows
//! its statements changed in total.

#[doc(hidden)]
pub mod columns;
mod ddl;
mod meta;
#[doc(hidden)]
pub mod tsql;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_schema::{Schema, SchemaRef};
use async_trait::async_trait;
use futures::StreamExt;
use platypus_core::{
    CancelHandle, Canceller, CatalogColumn, CatalogForeignKey, Connection, ConnectionConfig,
    DbError, Driver, DriverKind, Error, ExecOptions, ExecResult, MetadataMatch, ObjectMeta,
    ObjectRef, Property, RecordBatchStream, ReferencingKey, Result, SchemaMeta, ServerInfo,
    SslMode, TableDetails, TransactionState, TxTracker,
};
use tiberius::{AuthMethod, Client, EncryptionLevel, QueryItem, Row};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, OwnedMutexGuard, mpsc, oneshot};
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};
use tokio_util::sync::CancellationToken;

use crate::columns::{ColumnBuilder, ColumnSpec, column_builder};

type Session = Client<Compat<TcpStream>>;

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

const DEFAULT_PORT: u16 = 1433;

#[derive(Debug, Default, Clone, Copy)]
pub struct MssqlDriver;

#[async_trait]
impl Driver for MssqlDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::SqlServer
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let conn: Box<dyn Connection> = Box::new(MssqlConnection::connect(config).await?);
        platypus_core::run_init(&*conn, &config.init, self.kind()).await?;
        Ok(conn)
    }
}

#[derive(Debug)]
pub struct MssqlConnection {
    /// Locked by a statement until its result is fully read or cancelled.
    session: Arc<Mutex<Session>>,
    canceller: MssqlCanceller,
    server: ServerInfo,
    /// Manual mode sends `BEGIN TRANSACTION` before the first statement that needs one.
    tx: TxTracker,
    /// Set once an error shows the server connection is gone.
    closed: Arc<AtomicBool>,
}

/// The `tiberius` configuration of `config`, without the address to connect to.
fn session_config(config: &ConnectionConfig) -> tiberius::Config {
    let mut c = tiberius::Config::new();
    // `host` names the server for TLS even when the socket goes to a tunnel.
    c.host(&config.host);
    c.port(config.port.unwrap_or(DEFAULT_PORT));
    if !config.database.is_empty() {
        c.database(&config.database);
    }
    if let Some(name) = &config.application_name {
        c.application_name(name);
    }
    c.authentication(AuthMethod::sql_server(
        &config.user,
        config.password.as_ref().map_or("", |p| p.expose()),
    ));
    match config.ssl_mode {
        SslMode::Disable => c.encryption(EncryptionLevel::NotSupported),
        // Encrypts when the server can; servers usually have a self-signed certificate.
        SslMode::Prefer => {
            c.encryption(EncryptionLevel::On);
            c.trust_cert();
        }
        SslMode::Require => {
            c.encryption(EncryptionLevel::Required);
            c.trust_cert();
        }
        // The TLS library always checks the host name, so `verify-ca` is `verify-full`.
        SslMode::VerifyCa | SslMode::VerifyFull => {
            c.encryption(EncryptionLevel::Required);
            if let Some(path) = &config.ssl_root_cert {
                c.trust_cert_ca(path.to_string_lossy());
            }
        }
    }
    c
}

/// The error of the vendored `tiberius` when the client asks for TLS and the server has none.
const NO_SERVER_TLS: &str = "the server does not support encryption";

async fn login(session: tiberius::Config, host: &str, port: u16) -> tiberius::Result<Session> {
    let stream = TcpStream::connect((host, port)).await?;
    stream.set_nodelay(true)?;
    Client::connect(session, stream.compat_write()).await
}

/// Logs in, falling back to plaintext for `prefer` when the server has no TLS, and
/// following one redirection (Azure SQL gateways send clients to the database's node)
/// unless the connection goes through a tunnel, which leads to one address only.
async fn open(config: &ConnectionConfig) -> Result<Session> {
    let mut session = session_config(config);
    let (host, port) = match config.connect_addr {
        Some(addr) => (addr.ip().to_string(), addr.port()),
        None => (config.host.clone(), config.port.unwrap_or(DEFAULT_PORT)),
    };
    let mut result = login(session.clone(), &host, port).await;
    if config.ssl_mode == SslMode::Prefer
        && matches!(&result, Err(tiberius::error::Error::Tls(m)) if m == NO_SERVER_TLS)
    {
        session.encryption(EncryptionLevel::NotSupported);
        result = login(session.clone(), &host, port).await;
    }
    match result {
        Err(tiberius::error::Error::Routing { host, port }) if config.connect_addr.is_none() => {
            session.host(&host);
            session.port(port);
            login(session, &host, port).await
        }
        other => other,
    }
    .map_err(|e| connect_error(e, &host, port))
}

impl MssqlConnection {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self> {
        if config.driver != DriverKind::SqlServer {
            return Err(Error::Config(format!(
                "{:?} is not a SQL Server config",
                config.driver
            )));
        }
        let timeout = config.connect_timeout.unwrap_or(DEFAULT_CONNECT_TIMEOUT);
        let mut session = tokio::time::timeout(timeout, open(config))
            .await
            .map_err(|_| Error::Connect(format!("timed out after {}s", timeout.as_secs())))??;

        // `sql_variant` values are cast: their type differs from row to row.
        let row = session
            .simple_query(
                "SELECT CAST(SERVERPROPERTY('ProductVersion') AS NVARCHAR(128)), \
                 CAST(SERVERPROPERTY('EngineEdition') AS INT), \
                 CAST(CONNECTIONPROPERTY('encrypt_option') AS NVARCHAR(10))",
            )
            .await
            .map_err(|e| map_error(e, ""))?
            .into_row()
            .await
            .map_err(|e| map_error(e, ""))?;
        let text = |i: usize| -> String {
            row.as_ref()
                .and_then(|r| r.get::<&str, _>(i))
                .unwrap_or_default()
                .to_owned()
        };
        // 5: Azure SQL Database, 8: Azure SQL Managed Instance.
        let edition = row.as_ref().and_then(|r| r.get::<i32, _>(1));
        let product = if matches!(edition, Some(5 | 8)) {
            "Azure SQL"
        } else {
            "SQL Server"
        };

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            canceller: MssqlCanceller::default(),
            server: ServerInfo {
                product: product.to_owned(),
                version: text(0),
                encrypted: text(2).eq_ignore_ascii_case("TRUE"),
            },
            tx: TxTracker::default(),
            closed: Arc::new(AtomicBool::new(false)),
        })
    }
}

enum Started {
    Affected(u64),
    Rows(SchemaRef, mpsc::Receiver<Result<RecordBatch>>),
}

#[async_trait]
impl Connection for MssqlConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let prepared = tsql::prepare(sql);
        let Some(sql) = prepared.sql else {
            return Ok(ExecResult::Affected { rows: 0 });
        };
        let mut session = Arc::clone(&self.session).lock_owned().await;
        if self.tx.needs_begin(prepared.effect) {
            run_simple(&mut session, "BEGIN TRANSACTION", &self.closed).await?;
            self.tx.set_open(true);
        }
        let result = self.start(session, sql.into_owned(), options).await;
        // DDL is transactional in SQL Server.
        self.tx.after(prepared.effect, false);
        result
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(self.canceller.clone())
    }

    fn server_info(&self) -> &ServerInfo {
        &self.server
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
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

    async fn object_properties(&self, object: &ObjectRef<'_>) -> Result<Vec<Property>> {
        meta::object_properties(self, object).await
    }

    async fn databases(&self) -> Result<Vec<String>> {
        meta::databases(self).await
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
        self.end_transaction("IF @@TRANCOUNT > 0 COMMIT TRANSACTION")
            .await
    }

    async fn rollback(&self) -> Result<()> {
        self.end_transaction("IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION")
            .await
    }

    fn capabilities(&self) -> platypus_core::Capabilities {
        platypus_core::Capabilities {
            transactions: true,
            cancel: true,
            object_ddl: true,
            multiple_databases: true,
            metadata_search: true,
            ..Default::default()
        }
    }
}

impl MssqlConnection {
    /// Ends the open transaction; the server may have rolled it back already (some errors
    /// do), hence the `@@TRANCOUNT` check.
    async fn end_transaction(&self, sql: &str) -> Result<()> {
        let mut session = self.session.lock().await;
        if !self.tx.state().open {
            return Ok(());
        }
        let result = run_simple(&mut session, sql, &self.closed).await;
        self.tx.set_open(false);
        result
    }

    async fn start(
        &self,
        session: OwnedMutexGuard<Session>,
        sql: String,
        options: &ExecOptions,
    ) -> Result<ExecResult> {
        let (reply, started) = oneshot::channel();
        let statement = Statement {
            sql,
            batch_size: options.batch_size.max(1),
            closed: Arc::clone(&self.closed),
        };
        let canceller = self.canceller.clone();
        tokio::spawn(async move { statement.run(session, reply, &canceller).await });
        match started.await.map_err(|_| Error::Closed)?? {
            Started::Affected(rows) => Ok(ExecResult::Affected { rows }),
            Started::Rows(schema, mut rx) => {
                let stream = futures::stream::poll_fn(move |cx| rx.poll_recv(cx));
                Ok(ExecResult::Rows(RecordBatchStream::new(
                    schema,
                    stream.boxed(),
                )))
            }
        }
    }
}

/// Runs `sql` to the end, discarding its results.
async fn run_simple(session: &mut Session, sql: &str, closed: &AtomicBool) -> Result<()> {
    let fail = |e| lost(map_error(e, sql), closed);
    session
        .simple_query(sql)
        .await
        .map_err(fail)?
        .into_results()
        .await
        .map_err(fail)?;
    Ok(())
}

/// How a statement's task ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The response was read to its end.
    Finished,
    /// Cancelled, abandoned by its consumer, or failed while rows were coming: the rest of
    /// the batch must be stopped with an attention.
    Interrupted,
}

struct Statement {
    sql: String,
    batch_size: usize,
    closed: Arc<AtomicBool>,
}

impl Statement {
    fn fail(&self, e: tiberius::error::Error) -> Error {
        lost(map_error(e, &self.sql), &self.closed)
    }

    /// Sends the batch, answers with its kind, then streams the rows of its first result.
    /// The session is clean when the lock is released.
    async fn run(
        self,
        mut session: OwnedMutexGuard<Session>,
        reply: oneshot::Sender<Result<Started>>,
        canceller: &MssqlCanceller,
    ) {
        if let Err(e) = session.send_batch(self.sql.as_str()).await {
            let _ = reply.send(Err(self.fail(e)));
            return;
        }
        let token = canceller.arm();
        let outcome = self.read(&mut session, reply, &token).await;
        canceller.disarm(&token);
        if outcome == Outcome::Interrupted && session.cancel().await.is_err() {
            self.closed.store(true, Ordering::Release);
        }
    }

    async fn read(
        &self,
        session: &mut Session,
        reply: oneshot::Sender<Result<Started>>,
        cancelled: &CancellationToken,
    ) -> Outcome {
        let mut results = tokio::select! {
            biased;
            () = cancelled.cancelled() => {
                let _ = reply.send(Err(Error::Cancelled));
                return Outcome::Interrupted;
            }
            results = session.batch_results() => match results {
                Ok(results) => results,
                Err(e) => {
                    let _ = reply.send(Err(self.fail(e)));
                    return Outcome::Finished;
                }
            },
        };
        let columns = tokio::select! {
            biased;
            () = cancelled.cancelled() => {
                let _ = reply.send(Err(Error::Cancelled));
                return Outcome::Interrupted;
            }
            item = results.next() => match item {
                Some(Ok(QueryItem::Metadata(meta))) => meta.columns().to_vec(),
                Some(Ok(QueryItem::Row(_))) => {
                    let _ = reply.send(Err(Error::Other("rows before their columns".to_owned())));
                    return Outcome::Interrupted;
                }
                Some(Err(e)) => {
                    let _ = reply.send(Err(self.fail(e)));
                    return Outcome::Finished;
                }
                None => {
                    let _ = reply.send(Ok(Started::Affected(results.rows_affected())));
                    return Outcome::Finished;
                }
            },
        };

        let (fields, builders): (Vec<_>, Vec<_>) = columns
            .iter()
            .map(|c| column_builder(&ColumnSpec::from(c)))
            .unzip();
        let schema: SchemaRef = Arc::new(Schema::new(fields));
        let (tx, rx) = mpsc::channel(1);
        if reply
            .send(Ok(Started::Rows(Arc::clone(&schema), rx)))
            .is_err()
        {
            return Outcome::Interrupted;
        }
        let mut rows = RowSink {
            schema,
            builders,
            len: 0,
            batch_size: self.batch_size,
            tx,
        };
        self.stream(&mut results, &mut rows, cancelled).await
    }

    /// Streams the rows of the first result set and reads the further ones to the end.
    async fn stream(
        &self,
        results: &mut tiberius::QueryStream<'_>,
        rows: &mut RowSink,
        cancelled: &CancellationToken,
    ) -> Outcome {
        let mut first = true;
        loop {
            let item = tokio::select! {
                biased;
                () = cancelled.cancelled() => {
                    rows.send(Err(Error::Cancelled), cancelled).await;
                    return Outcome::Interrupted;
                }
                () = rows.tx.closed() => return Outcome::Interrupted,
                item = results.next() => item,
            };
            match item {
                Some(Ok(QueryItem::Row(row))) if first => {
                    if let Err(e) = rows.append(&row) {
                        rows.send(Err(e), cancelled).await;
                        return Outcome::Interrupted;
                    }
                    if rows.len == rows.batch_size && !rows.flush(cancelled).await {
                        return Outcome::Interrupted;
                    }
                }
                Some(Ok(QueryItem::Row(_))) => {}
                Some(Ok(QueryItem::Metadata(_))) => {
                    if first && !rows.flush(cancelled).await {
                        return Outcome::Interrupted;
                    }
                    first = false;
                }
                Some(Err(e)) => {
                    if !first || rows.flush(cancelled).await {
                        rows.send(Err(self.fail(e)), cancelled).await;
                    }
                    return Outcome::Finished;
                }
                None => {
                    if first {
                        rows.flush(cancelled).await;
                    }
                    return Outcome::Finished;
                }
            }
        }
    }
}

/// The rows of the first result set on their way to the consumer.
struct RowSink {
    schema: SchemaRef,
    builders: Vec<ColumnBuilder>,
    len: usize,
    batch_size: usize,
    tx: mpsc::Sender<Result<RecordBatch>>,
}

impl RowSink {
    fn append(&mut self, row: &Row) -> Result<()> {
        for (i, (builder, (_, value))) in self.builders.iter_mut().zip(row.cells()).enumerate() {
            builder.append(value).map_err(|message| {
                let field = self.schema.field(i);
                Error::Decode {
                    column: field.name().clone(),
                    db_type: field
                        .metadata()
                        .get(platypus_core::META_DB_TYPE)
                        .cloned()
                        .unwrap_or_default(),
                    message,
                }
            })?;
        }
        self.len += 1;
        Ok(())
    }

    /// Sends the rows appended so far; `false` when the consumer is gone or cancelled.
    async fn flush(&mut self, cancelled: &CancellationToken) -> bool {
        if self.len == 0 {
            return true;
        }
        self.len = 0;
        let arrays = self
            .builders
            .iter_mut()
            .map(ColumnBuilder::finish)
            .collect();
        let batch = RecordBatch::try_new(Arc::clone(&self.schema), arrays).map_err(Error::from);
        self.send(batch, cancelled).await
    }

    /// Waits for the consumer to take `item`; `false` when it is gone or cancelled.
    async fn send(&self, item: Result<RecordBatch>, cancelled: &CancellationToken) -> bool {
        tokio::select! {
            biased;
            () = cancelled.cancelled() => {
                let _ = self.tx.try_send(Err(Error::Cancelled));
                false
            }
            sent = self.tx.send(item) => sent.is_ok(),
        }
    }
}

/// Cancels the statement running on the connection, if any, by waking its task, which
/// sends the attention: the session cannot be reached from another task.
#[derive(Debug, Clone, Default)]
struct MssqlCanceller {
    running: Arc<std::sync::Mutex<Option<CancellationToken>>>,
}

impl MssqlCanceller {
    /// A token for the statement that starts reading now.
    fn arm(&self) -> CancellationToken {
        let token = CancellationToken::new();
        *self.running.lock().unwrap_or_else(PoisonError::into_inner) = Some(token.clone());
        token
    }

    /// The statement of `token` is over: a later cancel must not reach the next one.
    fn disarm(&self, token: &CancellationToken) {
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        if running
            .as_ref()
            .is_some_and(|t| t.is_cancelled() || std::ptr::eq(t, token))
        {
            *running = None;
        }
        drop(running);
    }
}

#[async_trait]
impl Canceller for MssqlCanceller {
    async fn cancel(&self) -> Result<()> {
        let token = self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(token) = token {
            token.cancel();
        }
        Ok(())
    }
}

fn connect_error(e: tiberius::error::Error, host: &str, port: u16) -> Error {
    match e {
        tiberius::error::Error::Server(_) => map_error(e, ""),
        other => Error::Connect(format!("{host}:{port}: {other}")),
    }
}

/// Records errors that mean the connection is gone, so the session can reconnect.
fn lost(error: Error, closed: &AtomicBool) -> Error {
    if matches!(error, Error::Connect(_) | Error::Closed) {
        closed.store(true, Ordering::Release);
    }
    error
}

/// Maps an error raised by `sql`, locating it in the statement.
fn map_error(e: tiberius::error::Error, sql: &str) -> Error {
    use tiberius::error::Error as E;
    match e {
        E::Server(t) => {
            let (detail, position) = if t.procedure().is_empty() {
                (
                    format!("Severity {}, state {}", t.class(), t.state()),
                    tsql::error_position(sql, t.line(), t.message()),
                )
            } else {
                let detail = format!(
                    "In {} at line {} (severity {}, state {})",
                    t.procedure(),
                    t.line(),
                    t.class(),
                    t.state()
                );
                (detail, None)
            };
            Error::Database(DbError {
                code: Some(t.code().to_string()),
                message: t.message().to_owned(),
                detail: Some(detail),
                hint: None,
                position,
            })
        }
        // The response can no longer be followed: the session is unusable.
        E::Io { .. } | E::Protocol(_) | E::Tls(_) | E::Routing { .. } => {
            Error::Connect(e.to_string())
        }
        other => Error::Other(other.to_string()),
    }
}
