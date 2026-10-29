//! MySQL/MariaDB driver built on `mysql_async`.
//!
//! A statement runs on a task that owns the connection for its whole lifetime and sends
//! batches through a one-slot channel, so rows are read from the socket only as fast as
//! they are consumed. Queries are cancelled with `KILL QUERY` from a second connection.

mod admin;
mod bulk;
#[doc(hidden)]
pub mod columns;
mod explain;
pub use explain::read_plan;
mod meta;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_schema::{Schema, SchemaRef};
use async_trait::async_trait;
use futures::StreamExt;
use mysql_async::prelude::Queryable;
use mysql_async::{ClientIdentity, Conn, DriverError, Opts, OptsBuilder, Row, SslOpts, Value};
use platypus_core::{
    Auth, CancelHandle, Canceller, CatalogColumn, Connection, ConnectionConfig, DbError, Driver,
    DriverKind, Error, ExecOptions, ExecResult, ObjectMeta, RecordBatchStream, ReferencingKey,
    Result, SchemaMeta, ServerInfo, SslMode, TableDetails, TransactionState, TxTracker, classify,
};
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::columns::{ColumnBuilder, ColumnSpec, SESSION_TIME_ZONE, column_builder};

/// `ER_QUERY_INTERRUPTED`: the statement was stopped by `KILL QUERY`.
const QUERY_INTERRUPTED: u16 = 1317;

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long [`Connection::check_alive`] waits for the server's answer.
const PING_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Default, Clone, Copy)]
pub struct MySqlDriver;

#[async_trait]
impl Driver for MySqlDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::MySql
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let conn: Box<dyn Connection> = Box::new(MySqlConnection::connect(config).await?);
        platypus_core::run_init(&*conn, &config.init, self.kind()).await?;
        Ok(conn)
    }
}

#[derive(Debug)]
pub struct MySqlConnection {
    /// Locked by a statement until its result is fully read or drained.
    conn: Arc<Mutex<Conn>>,
    canceller: MySqlCanceller,
    server: ServerInfo,
    /// With `NO_BACKSLASH_ESCAPES`, `\` is an ordinary character in string literals.
    no_backslash_escapes: bool,
    /// The server takes `LOAD DATA LOCAL`, for bulk loads.
    local_infile: bool,
    /// Manual mode is `SET autocommit = 0`; the server opens transactions by itself.
    tx: TxTracker,
    /// Set once an error shows the server connection is gone.
    closed: Arc<AtomicBool>,
    /// Whether the session's time zone is UTC, so that `TIMESTAMP` values are instants of
    /// it; read again after a statement that names `time_zone`.
    utc: Arc<AtomicBool>,
    messages: Messages,
}

fn ssl_opts(config: &ConnectionConfig) -> Result<Option<SslOpts>> {
    let roots = || {
        config
            .ssl_root_cert
            .iter()
            .map(|p| p.clone().into())
            .collect()
    };
    let identity = match (&config.ssl_client_cert, &config.ssl_client_key) {
        (Some(cert), Some(key)) => {
            Some(ClientIdentity::new(cert.clone().into(), key.clone().into()))
        }
        (None, None) => None,
        _ => {
            return Err(Error::Config(
                "a client certificate needs both the certificate and its key".to_owned(),
            ));
        }
    };
    let base = SslOpts::default().with_client_identity(identity);
    Ok(match config.ssl_mode {
        SslMode::Disable => None,
        SslMode::Prefer | SslMode::Require => Some(
            base.with_danger_accept_invalid_certs(true)
                .with_danger_skip_domain_validation(true),
        ),
        SslMode::VerifyCa => Some(
            base.with_root_certs(roots())
                .with_danger_skip_domain_validation(true),
        ),
        SslMode::VerifyFull => Some(base.with_root_certs(roots())),
    })
}

fn opts(config: &ConnectionConfig, ssl: Option<SslOpts>) -> Opts {
    // Through a tunnel the certificate still names the server, not the tunnel's address.
    let (address, port, ssl) = match config.connect_addr {
        Some(addr) => (
            addr.ip().to_string(),
            addr.port(),
            ssl.map(|s| s.with_danger_tls_hostname_override(Some(config.host.clone()))),
        ),
        None => (config.host.clone(), config.port.unwrap_or(3306), ssl),
    };
    let mut builder = OptsBuilder::default()
        .ip_or_hostname(address)
        .tcp_port(port)
        .user(Some(config.user.clone()))
        .pass(config.password.as_ref().map(|p| p.expose().to_owned()))
        .db_name(Some(config.database.clone()).filter(|d| !d.is_empty()))
        .prefer_socket(false)
        .stmt_cache_size(0)
        // Row counts of UPDATE are the rows matched, as in other databases, not only the
        // rows whose value changed: the data editor checks that each change hit one row.
        .client_found_rows(true)
        .init(vec![format!("SET time_zone = '{SESSION_TIME_ZONE}'")])
        .ssl_opts(ssl)
        // IAM and Entra ID tokens go to the server as written (`mysql_clear_password`):
        // RDS, Cloud SQL and Azure ask for it, over TLS.
        .enable_cleartext_plugin(config.auth().is_ok_and(Auth::is_token));
    if let Some(name) = &config.application_name {
        builder = builder.connect_attribute("program_name", name.clone());
    }
    builder.into()
}

async fn open(opts: Opts, timeout: Duration) -> Result<Conn> {
    match tokio::time::timeout(timeout, Conn::new(opts)).await {
        Ok(result) => result.map_err(connect_error),
        Err(_) => Err(Error::Connect(format!(
            "timed out after {}s",
            timeout.as_secs()
        ))),
    }
}

impl MySqlConnection {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self> {
        if config.driver != DriverKind::MySql {
            return Err(Error::Config(format!(
                "{:?} is not a MySQL config",
                config.driver
            )));
        }
        let timeout = config.connect_timeout.unwrap_or(DEFAULT_CONNECT_TIMEOUT);
        let mut opts = opts(config, ssl_opts(config)?);
        let mut conn = match open(opts.clone(), timeout).await {
            // `prefer` falls back to plaintext when the server has no TLS, or only TLS this
            // client refuses (MySQL 5.7's images offer old versions and ciphers).
            Err(Error::Connect(message))
                if config.ssl_mode == SslMode::Prefer
                    && (message.contains(&DriverError::NoClientSslFlagFromServer.to_string())
                        || tls_refused(&message)) =>
            {
                opts = self::opts(config, None);
                open(opts.clone(), timeout).await?
            }
            other => other?,
        };

        let (version, sql_mode): (String, String) = conn
            .query_first("SELECT VERSION(), @@SESSION.sql_mode")
            .await
            .map_err(map_error)?
            .unwrap_or_default();
        let cipher: Option<(String, String)> = conn
            .query_first("SHOW SESSION STATUS LIKE 'Ssl_cipher'")
            .await
            .map_err(map_error)?;
        // A server without it (or that cannot say) refuses `LOAD DATA LOCAL`.
        let local_infile = conn
            .query_first::<i64, _>("SELECT @@GLOBAL.local_infile")
            .await
            .ok()
            .flatten()
            == Some(1);
        let product = if version.contains("MariaDB") {
            "MariaDB"
        } else {
            "MySQL"
        };

        Ok(Self {
            canceller: MySqlCanceller {
                opts,
                connection_id: conn.id(),
                timeout,
                requested: Arc::new(AtomicBool::new(false)),
            },
            conn: Arc::new(Mutex::new(conn)),
            server: ServerInfo {
                product: product.to_owned(),
                version,
                encrypted: cipher.is_some_and(|(_, c)| !c.is_empty()),
            },
            no_backslash_escapes: sql_mode.contains("NO_BACKSLASH_ESCAPES"),
            local_infile,
            tx: TxTracker::default(),
            closed: Arc::new(AtomicBool::new(false)),
            messages: Messages::default(),
            utc: Arc::new(AtomicBool::new(true)),
        })
    }

    /// A string literal for catalog queries.
    pub(crate) fn literal(&self, s: &str) -> String {
        let s = if self.no_backslash_escapes {
            s.to_owned()
        } else {
            s.replace('\\', "\\\\")
        };
        format!("'{}'", s.replace('\'', "''"))
    }
}

/// Whether the server offers no TLS this client takes (versions, ciphers), as rustls words
/// it; not a certificate the server asks for or refuses, which plaintext would only hide.
fn tls_refused(message: &str) -> bool {
    let lower = message.to_lowercase();
    [
        "handshakefailure",
        "protocolversion",
        "insufficientsecurity",
        "peer is incompatible",
    ]
    .iter()
    .any(|m| lower.contains(m))
}

/// Whether the server has what MySQL added in `version`: MariaDB and `TiDB` number their
/// versions apart (`TiDB` reports an old MySQL, `8.0.11-TiDB-v7.5.0`) and have it.
pub(crate) fn mysql_has(info: &ServerInfo, version: platypus_core::ServerVersion) -> bool {
    info.product == "MariaDB" || info.version.contains("TiDB") || info.at_least(version)
}

/// The warnings of the statements run, until `take_messages`.
type Messages = Arc<std::sync::Mutex<Vec<String>>>;

/// How a result of a statement starts, as its task tells the connection.
enum Started {
    Affected(u64),
    /// A result set: its rows, then what follows it.
    Rows(
        SchemaRef,
        mpsc::Receiver<Result<RecordBatch>>,
        oneshot::Receiver<Result<Option<Started>>>,
    ),
}

/// Where a task sends the next result of its statement; `None` when there is no other.
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
impl Connection for MySqlConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let effect = classify(sql);
        let result = self.start(sql, options).await;
        self.tx.after(effect, true);
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

    fn take_messages(&self) -> Vec<String> {
        std::mem::take(&mut *self.messages.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// `COM_PING`, which opens no transaction. The socket of a session the server closed
    /// (`wait_timeout`, `KILL`) fails only when used.
    async fn check_alive(&self) -> bool {
        if self.is_closed() {
            return false;
        }
        // A statement holds the connection: in use, so alive.
        let Ok(mut conn) = self.conn.try_lock() else {
            return true;
        };
        match tokio::time::timeout(PING_TIMEOUT, conn.ping()).await {
            Ok(Ok(())) => true,
            Ok(Err(e)) => {
                lost(map_error(e), &self.closed);
                !self.is_closed()
            }
            // Unanswered: the half-sent ping leaves the protocol out of step.
            Err(_) => {
                self.closed.store(true, Ordering::Release);
                false
            }
        }
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

    async fn explain(&self, sql: &str, analyze: bool) -> Result<platypus_core::Explain> {
        explain::explain(self, sql, analyze).await
    }

    async fn object_ddl(&self, object: &platypus_core::ObjectRef<'_>) -> Result<String> {
        meta::object_ddl(self, object).await
    }

    async fn object_properties(
        &self,
        object: &platypus_core::ObjectRef<'_>,
    ) -> Result<Vec<platypus_core::Property>> {
        meta::object_properties(self, object).await
    }

    async fn search_metadata(&self, needle: &str) -> Result<Vec<platypus_core::MetadataMatch>> {
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

    async fn top_queries(&self) -> Result<Vec<platypus_core::ServerQuery>> {
        admin::top_queries(self).await
    }

    async fn object_statistics(&self) -> Result<Vec<platypus_core::ObjectStatistics>> {
        admin::object_statistics(self).await
    }

    async fn maintain(
        &self,
        schema: &str,
        table: &str,
        operation: platypus_core::Maintenance,
    ) -> Result<Vec<String>> {
        admin::maintain(self, schema, table, operation).await
    }

    fn transaction(&self) -> TransactionState {
        self.tx.state()
    }

    async fn set_autocommit(&self, on: bool) -> Result<()> {
        self.tx.ensure_closed()?;
        let sql = if on {
            "SET autocommit = 1"
        } else {
            "SET autocommit = 0"
        };
        self.conn
            .lock()
            .await
            .query_drop(sql)
            .await
            .map_err(map_error)?;
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
            server_sessions: true,
            cancel_server_queries: true,
            server_statistics: true,
            maintenance: admin::MAINTENANCE,
            bulk_load: self.local_infile,
            ..Default::default()
        }
    }

    async fn bulk_load(&self, rows: &platypus_core::BulkRows<'_>) -> Result<u64> {
        bulk::load(self, rows).await
    }
}

impl MySqlConnection {
    async fn end_transaction(&self, sql: &str) -> Result<()> {
        let mut conn = self.conn.lock().await;
        if !self.tx.state().open {
            return Ok(());
        }
        let result = conn.query_drop(sql).await.map_err(map_error);
        self.tx.set_open(false);
        result
    }

    async fn start(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let mut conn = Arc::clone(&self.conn).lock_owned().await;
        let (reply, started) = oneshot::channel();
        let sql = sql.to_owned();
        // Read under the lock: a statement that changed the zone has updated it by now.
        let utc = Arc::clone(&self.utc);
        let task = Task {
            batch_size: options.batch_size.max(1),
            canceller: self.canceller.clone(),
            closed: Arc::clone(&self.closed),
            utc: utc.load(Ordering::Acquire),
            messages: Arc::clone(&self.messages),
        };
        tokio::spawn(async move {
            run(&mut conn, &sql, &task, reply).await;
            if names_time_zone(&sql)
                && let Some(in_utc) = session_in_utc(&mut conn).await
            {
                utc.store(in_utc, Ordering::Release);
            }
        });
        match started.await.map_err(|_| Error::Closed)?? {
            Some(started) => Ok(started.into_result()),
            None => Ok(ExecResult::Affected { rows: 0 }),
        }
    }
}

/// Runs one statement on the locked connection: answers with its first result, then streams
/// the rows of each result set in turn (a `CALL` may return several). A consumer that goes
/// away gets its query killed and the rest of it drained, so the connection is clean before
/// the lock is released; one that stops asking for the next result leaves the rest unread.
async fn run(conn: &mut Conn, sql: &str, task: &Task, reply: Reply) {
    task.canceller.requested.store(false, Ordering::Release);
    let fail = |e: mysql_async::Error| lost(map_error_in(e, sql), &task.closed);
    let mut result = match conn.query_iter(sql).await {
        Ok(result) => result,
        Err(e) => {
            let _ = reply.send(Err(fail(e)));
            return;
        }
    };
    if result.columns_ref().is_empty() {
        let rows = result.affected_rows();
        let drained = result.drop_result().await;
        task.keep_warnings(conn).await;
        let answer = match drained {
            // `KILL QUERY` ends `SLEEP()` and the like as if they were done.
            Ok(()) if task.canceller.requested.load(Ordering::Acquire) => Err(Error::Cancelled),
            Ok(()) => Ok(Some(Started::Affected(rows))),
            Err(e) => Err(fail(e)),
        };
        let _ = reply.send(answer);
        return;
    }
    let mut reply = Some(reply);
    let mut first = true;
    // The rows of the last result set end once the warnings are read: they come first.
    let mut last: Option<mpsc::Sender<Result<RecordBatch>>> = None;
    let mut abandoned = false;
    loop {
        let mut stream = match result.stream::<Row>().await {
            Ok(Some(stream)) => stream,
            Ok(None) => break,
            Err(e) => {
                let error = fail(e);
                match (last.take(), reply.take()) {
                    (_, Some(to)) => {
                        let _ = to.send(Err(error));
                    }
                    (Some(tx), None) => {
                        let _ = tx.send(Err(error)).await;
                    }
                    (None, None) => {}
                }
                break;
            }
        };
        // The OK packet that ends a `CALL`.
        if stream.columns_ref().is_empty() {
            continue;
        }
        let (fields, builders): (Vec<_>, Vec<_>) = stream
            .columns_ref()
            .iter()
            .map(|c| column_builder(&ColumnSpec::from(c), task.utc))
            .unzip();
        let schema: SchemaRef = Arc::new(Schema::new(fields));
        let Some(to) = reply.take() else {
            // Nobody asks for this result: read it to the end unseen.
            while let Some(Ok(_)) = stream.next().await {}
            continue;
        };
        let (tx, rx) = mpsc::channel(1);
        let (next, follows) = oneshot::channel();
        drop(last.take());
        if to
            .send(Ok(Some(Started::Rows(Arc::clone(&schema), rx, follows))))
            .is_err()
        {
            abandoned = first;
            if abandoned {
                break;
            }
            while let Some(Ok(_)) = stream.next().await {}
            continue;
        }
        first = false;
        reply = Some(next);
        let mut sink = Sink {
            schema,
            builders,
            tx,
        };
        abandoned = sink.stream(&mut stream, task, &fail).await;
        if abandoned {
            break;
        }
        last = Some(sink.tx);
    }

    if abandoned {
        let _ = task.canceller.cancel().await;
    }
    // Reads whatever is left; errors (such as the one caused by the kill) only matter to a
    // consumer that is gone.
    let _ = result.drop_result().await;
    if !abandoned {
        task.keep_warnings(conn).await;
    }
    drop(last);
    if let Some(to) = reply.take() {
        let _ = to.send(Ok(None));
    }
}

/// What a statement's task needs besides the connection.
struct Task {
    batch_size: usize,
    canceller: MySqlCanceller,
    closed: Arc<AtomicBool>,
    /// The session's zone is UTC (see `columns`).
    utc: bool,
    messages: Messages,
}

impl Task {
    /// Keeps the warnings of the statement just run for `take_messages`.
    async fn keep_warnings(&self, conn: &mut Conn) {
        if conn.get_warnings() == 0 {
            return;
        }
        let Ok(rows) = conn
            .query::<(String, Option<u32>, String), _>("SHOW WARNINGS")
            .await
        else {
            return;
        };
        self.messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(rows.into_iter().map(|(level, code, message)| match code {
                Some(code) => format!("{level} {code}: {message}"),
                None => format!("{level}: {message}"),
            }));
    }
}

/// The rows of a result set on their way to the consumer.
struct Sink {
    schema: SchemaRef,
    builders: Vec<ColumnBuilder>,
    tx: mpsc::Sender<Result<RecordBatch>>,
}

impl Sink {
    /// Sends the rows of `stream` in batches; `true` when the consumer went away or the
    /// statement must be killed.
    async fn stream(
        &mut self,
        stream: &mut (impl futures::Stream<Item = mysql_async::Result<Row>> + Unpin),
        task: &Task,
        fail: &impl Fn(mysql_async::Error) -> Error,
    ) -> bool {
        let mut len = 0;
        loop {
            let row = tokio::select! {
                () = self.tx.closed() => return true,
                row = stream.next() => row,
            };
            // `last`: nothing more will be sent; `kill`: the server is still sending rows.
            let (batch, last, kill) = match row {
                Some(Ok(row)) => match append_row(&mut self.builders, &self.schema, &row) {
                    Ok(()) => {
                        len += 1;
                        if len < task.batch_size {
                            continue;
                        }
                        (finish(&mut self.builders, &self.schema), false, false)
                    }
                    Err(e) => (Err(e), true, true),
                },
                Some(Err(e)) => (Err(fail(e)), true, false),
                None if len > 0 => (finish(&mut self.builders, &self.schema), true, false),
                None => return false,
            };
            len = 0;
            // `KILL QUERY` ends `SELECT SLEEP()` with a row, as if it were done.
            let batch = if task.canceller.requested.load(Ordering::Acquire) {
                Err(Error::Cancelled)
            } else {
                batch
            };
            let cancelled = batch.is_err();
            if self.tx.send(batch).await.is_err() || kill {
                return true;
            }
            if last || cancelled {
                return false;
            }
        }
    }
}

fn append_row(builders: &mut [ColumnBuilder], schema: &Schema, row: &Row) -> Result<()> {
    for (i, builder) in builders.iter_mut().enumerate() {
        let value = match row.as_ref(i) {
            None | Some(Value::NULL) => None,
            Some(Value::Bytes(bytes)) => Some(bytes.as_slice()),
            Some(other) => {
                // The text protocol only produces bytes; anything else is a library change.
                return Err(Error::Other(format!(
                    "unexpected value {other:?} in text protocol"
                )));
            }
        };
        builder.append(value).map_err(|message| {
            let field = schema.field(i);
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
    Ok(())
}

fn finish(builders: &mut [ColumnBuilder], schema: &SchemaRef) -> Result<RecordBatch> {
    let arrays = builders.iter_mut().map(ColumnBuilder::finish).collect();
    Ok(RecordBatch::try_new(Arc::clone(schema), arrays)?)
}

#[derive(Debug, Clone)]
struct MySqlCanceller {
    opts: Opts,
    connection_id: u32,
    timeout: Duration,
    /// A cancel was asked during the running statement, which `KILL QUERY` may end as if
    /// it were done (`SLEEP()` returns 1): it reports itself cancelled.
    requested: Arc<AtomicBool>,
}

#[async_trait]
impl Canceller for MySqlCanceller {
    async fn cancel(&self) -> Result<()> {
        self.requested.store(true, Ordering::Release);
        let mut conn = open(self.opts.clone(), self.timeout).await?;
        let killed = conn
            .query_drop(format!("KILL QUERY {}", self.connection_id))
            .await
            .map_err(map_error);
        let _ = conn.disconnect().await;
        killed
    }
}

fn connect_error(e: mysql_async::Error) -> Error {
    match e {
        mysql_async::Error::Server(_) => map_error(e),
        other => Error::Connect(platypus_core::without_repeats(&other.to_string())),
    }
}

/// Records errors that mean the connection is gone, so the session can reconnect.
/// Whether `sql` may set the session's time zone.
fn names_time_zone(sql: &str) -> bool {
    sql.to_ascii_lowercase().contains("time_zone")
}

/// Whether the session's time zone is UTC (`SYSTEM` when the server's is); `None` when it
/// cannot be read.
async fn session_in_utc(conn: &mut Conn) -> Option<bool> {
    let (session, system): (String, String) = conn
        .query_first("SELECT @@session.time_zone, @@system_time_zone")
        .await
        .ok()
        .flatten()?;
    let zone = if session.eq_ignore_ascii_case("SYSTEM") {
        system
    } else {
        session
    };
    Some(columns::is_utc(&zone))
}

fn lost(error: Error, closed: &AtomicBool) -> Error {
    if matches!(error, Error::Connect(_) | Error::Closed) {
        closed.store(true, Ordering::Release);
    }
    error
}

fn map_error(e: mysql_async::Error) -> Error {
    map_error_in(e, "")
}

/// Maps an error raised by `sql`, locating syntax errors in it.
fn map_error_in(e: mysql_async::Error, sql: &str) -> Error {
    match e {
        mysql_async::Error::Server(s) if s.code == QUERY_INTERRUPTED => Error::Cancelled,
        mysql_async::Error::Server(s) => Error::Database(DbError {
            code: Some(s.code.to_string()),
            position: error_position(sql, &s.message),
            detail: Some(format!("SQLSTATE {}", s.state)),
            message: s.message,
            hint: None,
        }),
        mysql_async::Error::Io(e) => Error::Connect(platypus_core::without_repeats(&e.to_string())),
        mysql_async::Error::Driver(DriverError::ConnectionClosed) => Error::Closed,
        other => Error::Other(other.to_string()),
    }
}

/// MySQL reports syntax errors as `... near 'rest of the query' at line N`; the quoted
/// text (cut to 80 characters by the server) is the part of `sql` that failed to parse.
fn error_position(sql: &str, message: &str) -> Option<u32> {
    let start = message.find(" near '")? + " near '".len();
    let end = message.rfind("' at line ")?;
    let snippet = message.get(start..end)?;
    let byte = if snippet.is_empty() {
        sql.trim_end().len()
    } else {
        sql.find(snippet)?
    };
    u32::try_from(sql[..byte].chars().count() + 1).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_tls_refusals_from_other_errors() {
        assert!(tls_refused(
            "Input/output error: received fatal alert: HandshakeFailure"
        ));
        assert!(tls_refused("peer is incompatible: ProtocolVersion"));
        assert!(!tls_refused("Access denied for user 'x'@'%'"));
        assert!(!tls_refused("received fatal alert: CertificateRequired"));
    }

    #[test]
    fn tidb_and_mariadb_have_what_mysql_added_later() {
        let info = |product: &str, version: &str| ServerInfo {
            product: product.to_owned(),
            version: version.to_owned(),
            encrypted: false,
        };
        let v = platypus_core::ServerVersion::new(8, 0, 16);
        assert!(!mysql_has(&info("MySQL", "5.7.44-log"), v));
        assert!(mysql_has(&info("MySQL", "8.0.36"), v));
        assert!(mysql_has(&info("MySQL", "8.0.11-TiDB-v7.5.0"), v));
        assert!(mysql_has(&info("MariaDB", "10.6.21-MariaDB"), v));
    }

    #[test]
    fn client_certificate_needs_cert_and_key() {
        let mut config = ConnectionConfig::new(DriverKind::MySql);
        config.ssl_mode = SslMode::Require;
        assert!(ssl_opts(&config).unwrap().is_some());
        config.ssl_client_cert = Some("client.crt".into());
        assert!(ssl_opts(&config).is_err());
        config.ssl_client_key = Some("client.key".into());
        assert!(ssl_opts(&config).unwrap().is_some());
    }

    #[test]
    fn locates_syntax_errors() {
        let sql = "select * fromm t";
        let message = "You have an error in your SQL syntax; check the manual that \
                       corresponds to your MySQL server version for the right syntax to use \
                       near 'fromm t' at line 1";
        assert_eq!(error_position(sql, message), Some(10));
        let end = "... near '' at line 1";
        assert_eq!(error_position("select 1 +", end), Some(11));
        assert_eq!(error_position(sql, "Unknown column 'x'"), None);
    }
}
