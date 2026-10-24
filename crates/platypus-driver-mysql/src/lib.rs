//! MySQL/MariaDB driver built on `mysql_async`.
//!
//! A statement runs on a task that owns the connection for its whole lifetime and sends
//! batches through a one-slot channel, so rows are read from the socket only as fast as
//! they are consumed. Queries are cancelled with `KILL QUERY` from a second connection.

#[doc(hidden)]
pub mod columns;
mod explain;
mod meta;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_schema::{Schema, SchemaRef};
use async_trait::async_trait;
use futures::StreamExt;
use mysql_async::prelude::Queryable;
use mysql_async::{ClientIdentity, Conn, DriverError, Opts, OptsBuilder, Row, SslOpts, Value};
use platypus_core::{
    CancelHandle, Canceller, CatalogColumn, Connection, ConnectionConfig, DbError, Driver,
    DriverKind, Error, ExecOptions, ExecResult, ObjectMeta, RecordBatchStream, ReferencingKey,
    Result, SchemaMeta, ServerInfo, SslMode, TableDetails, TransactionState, TxTracker, classify,
};
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::columns::{ColumnBuilder, ColumnSpec, SESSION_TIME_ZONE, column_builder};

/// `ER_QUERY_INTERRUPTED`: the statement was stopped by `KILL QUERY`.
const QUERY_INTERRUPTED: u16 = 1317;

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

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
    /// Manual mode is `SET autocommit = 0`; the server opens transactions by itself.
    tx: TxTracker,
    /// Set once an error shows the server connection is gone.
    closed: Arc<AtomicBool>,
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
        .ssl_opts(ssl);
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
            // `prefer` falls back to plaintext when the server has no TLS.
            Err(Error::Connect(message))
                if config.ssl_mode == SslMode::Prefer
                    && message == DriverError::NoClientSslFlagFromServer.to_string() =>
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
            },
            conn: Arc::new(Mutex::new(conn)),
            server: ServerInfo {
                product: product.to_owned(),
                version,
                encrypted: cipher.is_some_and(|(_, c)| !c.is_empty()),
            },
            no_backslash_escapes: sql_mode.contains("NO_BACKSLASH_ESCAPES"),
            tx: TxTracker::default(),
            closed: Arc::new(AtomicBool::new(false)),
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

enum Started {
    Affected(u64),
    Rows(SchemaRef, mpsc::Receiver<Result<RecordBatch>>),
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
            ..Default::default()
        }
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
        let batch_size = options.batch_size.max(1);
        let canceller = self.canceller.clone();
        let closed = Arc::clone(&self.closed);
        tokio::spawn(async move {
            run(&mut conn, &sql, batch_size, reply, &canceller, &closed).await;
        });
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

/// Runs one statement on the locked connection: answers with its kind, then streams the
/// rows. A consumer that goes away gets its query killed and the rest of it drained, so
/// the connection is clean before the lock is released.
async fn run(
    conn: &mut Conn,
    sql: &str,
    batch_size: usize,
    reply: oneshot::Sender<Result<Started>>,
    canceller: &MySqlCanceller,
    closed: &AtomicBool,
) {
    let fail = |e: mysql_async::Error| lost(map_error_in(e, sql), closed);
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
        let _ = reply.send(drained.map(|()| Started::Affected(rows)).map_err(fail));
        return;
    }
    let mut stream = match result.stream::<Row>().await {
        Ok(Some(stream)) => stream,
        Ok(None) => {
            let _ = reply.send(Ok(Started::Affected(0)));
            return;
        }
        Err(e) => {
            let _ = reply.send(Err(fail(e)));
            return;
        }
    };

    let (fields, mut builders): (Vec<_>, Vec<_>) = stream
        .columns_ref()
        .iter()
        .map(|c| column_builder(&ColumnSpec::from(c)))
        .unzip();
    let schema: SchemaRef = Arc::new(Schema::new(fields));
    let (tx, rx) = mpsc::channel(1);
    let mut abandoned = reply
        .send(Ok(Started::Rows(Arc::clone(&schema), rx)))
        .is_err();

    let mut len = 0;
    while !abandoned {
        let row = tokio::select! {
            () = tx.closed() => {
                abandoned = true;
                break;
            }
            row = stream.next() => row,
        };
        // `last`: nothing more will be sent; `kill`: the server is still sending rows.
        let (batch, last, kill) = match row {
            Some(Ok(row)) => match append_row(&mut builders, &schema, &row) {
                Ok(()) => {
                    len += 1;
                    if len < batch_size {
                        continue;
                    }
                    (finish(&mut builders, &schema), false, false)
                }
                Err(e) => (Err(e), true, true),
            },
            Some(Err(e)) => (Err(fail(e)), true, false),
            None if len > 0 => (finish(&mut builders, &schema), true, false),
            None => break,
        };
        len = 0;
        abandoned = tx.send(batch).await.is_err() || kill;
        if last {
            break;
        }
    }

    if abandoned {
        let _ = canceller.cancel().await;
    }
    drop(stream);
    // Reads whatever is left, including further result sets; errors (such as the one
    // caused by the kill) only matter to a consumer that is gone.
    let _ = result.drop_result().await;
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
}

#[async_trait]
impl Canceller for MySqlCanceller {
    async fn cancel(&self) -> Result<()> {
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
        other => Error::Connect(other.to_string()),
    }
}

/// Records errors that mean the connection is gone, so the session can reconnect.
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
        mysql_async::Error::Io(e) => Error::Connect(e.to_string()),
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
