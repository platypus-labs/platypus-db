//! Microsoft SQL Server and Azure SQL driver built on `tiberius` (vendored with patches in
//! `vendor/tiberius`).
//!
//! A statement runs as a SQL batch on a task that owns the session for its whole lifetime
//! and sends batches of rows through a one-slot channel, so rows are read from the socket
//! only as fast as they are consumed. Cancelling, or dropping an unfinished stream, sends a
//! TDS attention: the server stops the batch and the session stays usable, with its
//! transaction, temporary tables and settings.
//!
//! A batch can return several result sets: `execute` streams the first one, and each is
//! followed by the next ([`platypus_core::NextResult`]); one nobody asks for is read to the end
//! unseen, so every statement of the batch runs and its errors are reported. A batch without
//! results reports the rows its statements changed in total. `PRINT` and other
//! informational messages are kept for [`Connection::take_messages`].

mod admin;
#[doc(hidden)]
pub mod columns;
mod ddl;
mod meta;
#[doc(hidden)]
pub mod tsql;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// How long [`Connection::check_alive`] waits for the server's answer.
const PING_TIMEOUT: Duration = Duration::from_secs(5);

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
    messages: Messages,
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
/// unless the connection goes through a tunnel, which leads to one address only. Returns
/// the session with whether it is encrypted: when the client asks for encryption the login
/// fails rather than going on in plaintext, so it is unless `prefer` fell back.
/// (`CONNECTIONPROPERTY` has no `encrypt_option`, and `sys.dm_exec_connections` needs
/// `VIEW SERVER STATE`.)
async fn open(config: &ConnectionConfig) -> Result<(Session, bool)> {
    let mut session = session_config(config);
    let (host, port) = match config.connect_addr {
        Some(addr) => (addr.ip().to_string(), addr.port()),
        None => (config.host.clone(), config.port.unwrap_or(DEFAULT_PORT)),
    };
    let mut encrypted = config.ssl_mode != SslMode::Disable;
    let mut result = login(session.clone(), &host, port).await;
    if config.ssl_mode == SslMode::Prefer
        && matches!(&result, Err(tiberius::error::Error::Tls(m)) if m == NO_SERVER_TLS)
    {
        session.encryption(EncryptionLevel::NotSupported);
        encrypted = false;
        result = login(session.clone(), &host, port).await;
    }
    let session = match result {
        Err(tiberius::error::Error::Routing { host, port }) if config.connect_addr.is_none() => {
            session.host(&host);
            session.port(port);
            login(session, &host, port).await
        }
        other => other,
    }
    .map_err(|e| connect_error(e, &host, port))?;
    Ok((session, encrypted))
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
        let (mut session, encrypted) = tokio::time::timeout(timeout, open(config))
            .await
            .map_err(|_| Error::Connect(format!("timed out after {}s", timeout.as_secs())))??;

        // `sql_variant` values are cast: their type differs from row to row.
        let row = session
            .simple_query(
                "SELECT CAST(SERVERPROPERTY('ProductVersion') AS NVARCHAR(128)), \
                 CAST(SERVERPROPERTY('EngineEdition') AS INT)",
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
                encrypted,
            },
            tx: TxTracker::default(),
            closed: Arc::new(AtomicBool::new(false)),
            messages: Messages::default(),
        })
    }
}

/// The informational messages of the statements run (`PRINT`, `RAISERROR` below severity
/// 11), until [`Connection::take_messages`] takes them.
type Messages = Arc<std::sync::Mutex<Vec<String>>>;

/// How a result of a batch starts, as its task tells the connection.
enum Started {
    Affected(u64),
    /// A result set: its rows, then what follows it.
    Rows(
        SchemaRef,
        mpsc::Receiver<Result<RecordBatch>>,
        oneshot::Receiver<Result<Option<Started>>>,
    ),
}

/// Where a task sends the next result of its batch; `None` when there is no other.
type Reply = oneshot::Sender<Result<Option<Started>>>;

impl Started {
    fn into_result(self) -> ExecResult {
        match self {
            Self::Affected(rows) => ExecResult::Affected { rows },
            Self::Rows(schema, mut rx, next) => {
                let stream = futures::stream::poll_fn(move |cx| rx.poll_recv(cx));
                let next: platypus_core::NextResult = Box::pin(async move {
                    match next.await {
                        Ok(Ok(Some(started))) => Ok(Some(started.into_result())),
                        Ok(Ok(None)) | Err(_) => Ok(None),
                        Ok(Err(e)) => Err(e),
                    }
                });
                ExecResult::Rows(
                    RecordBatchStream::new(schema, stream.boxed()).with_next(Some(next)),
                )
            }
        }
    }
}

#[async_trait]
impl Connection for MssqlConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let prepared = tsql::prepare(sql);
        let Some(sql) = prepared.sql else {
            return Ok(ExecResult::Affected { rows: 0 });
        };
        let mut session = Arc::clone(&self.session).lock_owned().await;
        if !self.tx.state().autocommit {
            // The server may have rolled the transaction back (a deadlock, `XACT_ABORT`),
            // or a nested `COMMIT` may have left one open.
            let open = trancount(&mut session, &self.closed).await? > 0;
            self.tx.set_open(open);
        }
        if self.tx.needs_begin(prepared.effect) {
            run_simple(&mut session, "BEGIN TRANSACTION", &self.closed).await?;
            self.tx.set_open(true);
        }
        let result = self.start(session, sql.into_owned(), options).await;
        // DDL is transactional in SQL Server.
        self.tx.after(prepared.effect, false);
        if prepared.effect == platypus_core::TxEffect::End
            && matches!(result, Ok(ExecResult::Affected { .. }))
        {
            // `COMMIT` closes one level of nested transactions only.
            let mut session = self.session.lock().await;
            self.tx
                .set_open(trancount(&mut session, &self.closed).await? > 0);
        }
        result
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(self.canceller.clone())
    }

    fn server_info(&self) -> &ServerInfo {
        &self.server
    }

    fn take_messages(&self) -> Vec<String> {
        std::mem::take(&mut *self.messages.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// `SELECT 1` on the session, outside the transaction tracking: in manual-commit mode
    /// the driver sends `BEGIN TRANSACTION` itself, so the probe opens none. A session
    /// killed on the server fails only when used.
    async fn check_alive(&self) -> bool {
        if self.is_closed() {
            return false;
        }
        // A statement holds the session: in use, so alive.
        let Ok(mut session) = self.session.try_lock() else {
            return true;
        };
        let probe = run_simple(&mut session, "SELECT 1", &self.closed);
        if tokio::time::timeout(PING_TIMEOUT, probe).await.is_err() {
            // Unanswered: the half-read response leaves the protocol out of step.
            self.closed.store(true, Ordering::Release);
        }
        !self.is_closed()
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

    async fn explain(&self, sql: &str, analyze: bool) -> Result<platypus_core::Explain> {
        if analyze {
            return Err(Error::Unsupported(
                "SQL Server plans with actual rows; Explain plan shows the estimated one"
                    .to_owned(),
            ));
        }
        self.showplan(sql).await
    }

    async fn search_metadata(&self, needle: &str) -> Result<Vec<MetadataMatch>> {
        meta::search_metadata(self, needle).await
    }

    async fn server_sessions(&self) -> Result<Vec<platypus_core::ServerSession>> {
        admin::sessions(self).await
    }

    async fn server_locks(&self) -> Result<Vec<platypus_core::ServerLock>> {
        admin::locks(self).await
    }

    async fn end_server_session(&self, id: &str, how: platypus_core::EndSession) -> Result<()> {
        admin::end(self, id, how).await
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
            explain: true,
            object_ddl: true,
            multiple_databases: true,
            metadata_search: true,
            server_sessions: true,
            cancel_server_queries: false,
            // Imports send their rows through OPENJSON (`platypus-transfer`), not a bulk copy.
            bulk_load: false,
        }
    }
}

/// The steps of `SHOWPLAN_TEXT`: each a line `|--Operation(...)`, indented by its depth; the
/// other lines repeat the statement.
fn showplan_steps<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    lines
        .filter(|line| line.contains("|--"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn showplan_tree(plan: &str) -> platypus_core::PlanNode {
    platypus_core::plan_from_text(plan, |line| {
        let at = line.find("|--")?;
        Some((line[..at].chars().count(), line[at + 3..].trim().to_owned()))
    })
}

/// A plan pasted or opened from a file: the text of `SET SHOWPLAN_TEXT ON` (or
/// `STATISTICS PROFILE`'s `StmtText` column), its steps drawn `|--Operation(...)`.
///
/// # Errors
/// When the text has no step.
pub fn read_plan(document: &str) -> Result<platypus_core::Explain> {
    let plan = showplan_steps(platypus_core::plain_plan(document).lines());
    if plan.is_empty() {
        return Err(Error::Other("no plan in the text".to_owned()));
    }
    let root = showplan_tree(&plan);
    Ok(platypus_core::Explain::read(
        platypus_core::ExplainFormat::Text,
        plan,
        root,
    ))
}

impl MssqlConnection {
    /// The estimated plan, from `SET SHOWPLAN_TEXT ON`: the statement is compiled, not run.
    async fn showplan(&self, sql: &str) -> Result<platypus_core::Explain> {
        let mut session = self.session.lock().await;
        run_simple(&mut session, "SET SHOWPLAN_TEXT ON", &self.closed).await?;
        let fail = |e| lost(map_error(e, sql), &self.closed);
        let sets = match session.simple_query(sql).await {
            Ok(stream) => stream.into_results().await.map_err(fail),
            Err(e) => Err(fail(e)),
        };
        let off = run_simple(&mut session, "SET SHOWPLAN_TEXT OFF", &self.closed).await;
        let sets = sets?;
        off?;
        let plan = showplan_steps(
            sets.iter()
                .flatten()
                .filter_map(|row| row.get::<&str, _>(0)),
        );
        if plan.is_empty() {
            return Err(Error::Other("the database returned no plan".to_owned()));
        }
        let root = showplan_tree(&plan);
        Ok(platypus_core::Explain::new(
            platypus_core::ExplainFormat::Text,
            plan,
            root,
            false,
        ))
    }

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
            messages: Arc::clone(&self.messages),
        };
        let canceller = self.canceller.clone();
        tokio::spawn(async move { statement.run(session, reply, &canceller).await });
        match started.await.map_err(|_| Error::Closed)?? {
            Some(started) => Ok(started.into_result()),
            None => Ok(ExecResult::Affected { rows: 0 }),
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

/// The session's `@@TRANCOUNT`: how many `BEGIN TRANSACTION`s are open.
async fn trancount(session: &mut Session, closed: &AtomicBool) -> Result<i32> {
    const SQL: &str = "SELECT @@TRANCOUNT";
    let fail = |e| lost(map_error(e, SQL), closed);
    let row = session
        .simple_query(SQL)
        .await
        .map_err(fail)?
        .into_row()
        .await
        .map_err(fail)?;
    Ok(row.and_then(|r| r.get::<i32, _>(0)).unwrap_or(0))
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
    messages: Messages,
}

impl Statement {
    fn fail(&self, e: tiberius::error::Error) -> Error {
        lost(map_error(e, &self.sql), &self.closed)
    }

    /// Moves the messages read so far where `take_messages` finds them, before the consumer
    /// hears of what came with them.
    fn keep_messages(&self, results: &tiberius::QueryStream<'_>) {
        let read = results.take_messages();
        if !read.is_empty() {
            self.messages
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend(read);
        }
    }

    /// Sends the batch, answers with its first result, then streams the rows of each result
    /// set in turn. The session is clean when the lock is released.
    async fn run(
        self,
        mut session: OwnedMutexGuard<Session>,
        reply: Reply,
        canceller: &MssqlCanceller,
    ) {
        // Armed before sending: a cancel while a long script is written stops it too.
        let (id, token) = canceller.arm();
        if let Err(e) = session.send_batch(self.sql.as_str()).await {
            canceller.disarm(id);
            let _ = reply.send(Err(self.fail(e)));
            return;
        }
        let outcome = self.read(&mut session, reply, &token).await;
        canceller.disarm(id);
        if outcome == Outcome::Interrupted && session.cancel().await.is_err() {
            self.closed.store(true, Ordering::Release);
        }
    }

    async fn read(
        &self,
        session: &mut Session,
        reply: Reply,
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
        self.stream(&mut results, reply, cancelled).await
    }

    /// Streams the batch's result sets one after the other: each goes to the consumer once
    /// it has asked for what follows the one before (`reply`). A consumer that stops asking
    /// leaves the rest to run to its end unread; one that drops a result set it is reading
    /// stops the batch.
    async fn stream(
        &self,
        results: &mut tiberius::QueryStream<'_>,
        reply: Reply,
        cancelled: &CancellationToken,
    ) -> Outcome {
        let mut reply = Some(reply);
        let mut sink: Option<RowSink> = None;
        let mut sets = 0;
        loop {
            let item = tokio::select! {
                biased;
                () = cancelled.cancelled() => {
                    match (&sink, reply.take()) {
                        (Some(rows), _) => { rows.send(Err(Error::Cancelled), cancelled).await; }
                        (None, Some(reply)) => { let _ = reply.send(Err(Error::Cancelled)); }
                        (None, None) => {}
                    }
                    return Outcome::Interrupted;
                }
                () = async {
                    match &sink {
                        Some(rows) => rows.tx.closed().await,
                        None => futures::future::pending().await,
                    }
                } => return Outcome::Interrupted,
                item = results.next() => item,
            };
            self.keep_messages(results);
            match item {
                Some(Ok(QueryItem::Metadata(meta))) => {
                    if let Some(mut rows) = sink.take()
                        && !rows.flush(cancelled).await
                    {
                        return Outcome::Interrupted;
                    }
                    sets += 1;
                    let Some(to) = reply.take() else {
                        continue;
                    };
                    let (fields, builders): (Vec<_>, Vec<_>) = meta
                        .columns()
                        .iter()
                        .map(|c| column_builder(&ColumnSpec::from(c)))
                        .unzip();
                    let schema: SchemaRef = Arc::new(Schema::new(fields));
                    let (tx, rx) = mpsc::channel(1);
                    let (next, follows) = oneshot::channel();
                    if to
                        .send(Ok(Some(Started::Rows(Arc::clone(&schema), rx, follows))))
                        .is_err()
                    {
                        // The first result's consumer is gone; a later one only stopped asking.
                        if sets == 1 {
                            return Outcome::Interrupted;
                        }
                        continue;
                    }
                    sink = Some(RowSink {
                        schema,
                        builders,
                        len: 0,
                        batch_size: self.batch_size,
                        tx,
                    });
                    reply = Some(next);
                }
                Some(Ok(QueryItem::Row(row))) => {
                    let Some(rows) = sink.as_mut() else {
                        continue;
                    };
                    if let Err(e) = rows.append(&row) {
                        rows.send(Err(e), cancelled).await;
                        return Outcome::Interrupted;
                    }
                    if rows.len == rows.batch_size && !rows.flush(cancelled).await {
                        return Outcome::Interrupted;
                    }
                }
                Some(Err(e)) => {
                    let error = self.fail(e);
                    if let Some(mut rows) = sink.take() {
                        if rows.flush(cancelled).await {
                            rows.send(Err(error), cancelled).await;
                        }
                    } else if let Some(to) = reply.take() {
                        let _ = to.send(Err(error));
                    }
                    return Outcome::Finished;
                }
                None => {
                    if let Some(mut rows) = sink.take() {
                        rows.flush(cancelled).await;
                    }
                    if let Some(to) = reply.take() {
                        let last = (sets == 0).then(|| Started::Affected(results.rows_affected()));
                        let _ = to.send(Ok(last));
                    }
                    return Outcome::Finished;
                }
            }
        }
    }
}

/// The rows of a result set on their way to the consumer.
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
    /// The running statement's token, with an id to tell it from the next one's.
    running: Arc<std::sync::Mutex<Option<(u64, CancellationToken)>>>,
    next_id: Arc<AtomicU64>,
}

impl MssqlCanceller {
    /// A token for the statement that starts reading now.
    fn arm(&self) -> (u64, CancellationToken) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let token = CancellationToken::new();
        *self.running.lock().unwrap_or_else(PoisonError::into_inner) = Some((id, token.clone()));
        (id, token)
    }

    /// The statement `id` is over: a later cancel must not reach the next one.
    fn disarm(&self, id: u64) {
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        if running.as_ref().is_some_and(|(armed, _)| *armed == id) {
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
        if let Some((_, token)) = token {
            token.cancel();
        }
        Ok(())
    }
}

fn connect_error(e: tiberius::error::Error, host: &str, port: u16) -> Error {
    match e {
        tiberius::error::Error::Server(_) => map_error(e, ""),
        other => Error::Connect(platypus_core::without_repeats(&format!(
            "{host}:{port}: {other}"
        ))),
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
            Error::Connect(platypus_core::without_repeats(&e.to_string()))
        }
        other => Error::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_pasted_showplan() {
        let text = "StmtText\n--------\nSELECT * FROM t WHERE a = 1\n  |--Clustered Index Scan(OBJECT:([db].[dbo].[t].[PK_t]), WHERE:([a]=(1)))\n       |--Index Seek(OBJECT:([db].[dbo].[t].[ix]))\n\n(2 rows affected)";
        let explain = read_plan(text).unwrap();
        assert!(!explain.analyzed);
        assert!(explain.root.operation.starts_with("Clustered Index Scan("));
        assert_eq!(explain.root.children.len(), 1);
        assert!(read_plan("SELECT 1").is_err());
    }
}
