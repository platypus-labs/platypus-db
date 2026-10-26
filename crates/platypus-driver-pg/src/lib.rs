//! PostgreSQL driver built on `tokio-postgres`.
//!
//! Rows are fetched in the binary protocol and decoded straight into Arrow builders, one
//! batch at a time: the next batch is only read from the socket when the consumer polls for
//! it, so memory stays flat regardless of result size.
//!
//! TLS follows libpq `sslmode` semantics, see [`tls`].

#[doc(hidden)]
pub mod columns;
mod cursor;
mod ddl;
#[doc(hidden)]
pub mod decode;
mod explain;
mod meta;
mod tls;

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_schema::Schema;
use async_trait::async_trait;
use futures::StreamExt;
use platypus_core::{
    CancelHandle, Canceller, CatalogColumn, Connection, ConnectionConfig, DbError, Driver,
    DriverKind, Error, ExecOptions, ExecResult, Executed, ObjectMeta, RecordBatchStream,
    ReferencingKey, Result, SchemaMeta, ServerInfo, TableDetails, TransactionState, TxEffect,
    TxTracker, classify, execute_each,
};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio_postgres::error::{ErrorPosition, SqlState};
use tokio_postgres::tls::MakeTlsConnect;
use tokio_postgres::types::{FromSql, ToSql, Type};
use tokio_postgres::{CancelToken, Client, NoTls, RowStream, SimpleQueryMessage, Socket};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::columns::{ColumnBuilder, column_builder};

#[derive(Debug, Default, Clone, Copy)]
pub struct PgDriver;

#[async_trait]
impl Driver for PgDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Postgres
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let conn: Box<dyn Connection> = Box::new(PgConnection::connect(config).await?);
        platypus_core::run_init(&*conn, &config.init, self.kind()).await?;
        Ok(conn)
    }
}

#[derive(Debug)]
pub struct PgConnection {
    client: Arc<Client>,
    canceller: PgCanceller,
    server: ServerInfo,
    /// Held for the lifetime of a statement, including its result stream, so a new
    /// statement never starts while an abandoned one is still being cancelled.
    busy: Arc<Mutex<()>>,
    tx: TxTracker,
    /// Names cursors, which carry results inside transactions.
    next_cursor: AtomicU64,
}

/// The server speaking the protocol, from `version()`: CockroachDB and YugabyteDB name
/// themselves there. CockroachDB's `server_version` is the PostgreSQL release it mimics,
/// so its own release replaces it.
fn product(full: Option<&str>, server_version: String) -> (&'static str, String) {
    let full = full.unwrap_or_default();
    if full.starts_with("CockroachDB") {
        let own = full
            .split(' ')
            .find(|w| w.starts_with('v'))
            .map(str::to_owned);
        ("CockroachDB", own.unwrap_or(server_version))
    } else if server_version.contains("-YB-") {
        ("YugabyteDB", server_version)
    } else {
        ("PostgreSQL", server_version)
    }
}

/// Whether `conn` is CockroachDB, which lacks some of PostgreSQL's catalog functions.
pub(crate) fn is_cockroach(conn: &dyn Connection) -> bool {
    conn.server_info().product == "CockroachDB"
}

impl PgConnection {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self> {
        if config.driver != DriverKind::Postgres {
            return Err(Error::Config(format!(
                "{:?} is not a PostgreSQL config",
                config.driver
            )));
        }
        let mut pg = tokio_postgres::Config::new();
        // Through a tunnel the socket goes to its local end; `host` still names the server
        // for TLS. Ports add up in tokio-postgres, so there is one call.
        let port = config
            .connect_addr
            .map_or(config.port.unwrap_or(5432), |a| a.port());
        pg.host(&config.host)
            .port(port)
            .dbname(&config.database)
            .user(&config.user);
        if let Some(addr) = config.connect_addr {
            pg.hostaddr(addr.ip());
        }
        if let Some(password) = &config.password {
            pg.password(password.expose());
        }
        if let Some(name) = &config.application_name {
            pg.application_name(name);
        }
        if let Some(timeout) = config.connect_timeout {
            pg.connect_timeout(timeout);
        }
        let (ssl_mode, tls) = tls::connector(config)?;
        pg.ssl_mode(ssl_mode);

        let client = match &tls {
            Some(tls) => spawn_connection(&pg, tls.clone()).await?,
            None => spawn_connection(&pg, NoTls).await?,
        };

        let row = client
            .simple_query(
                "SELECT current_setting('server_version'), \
                 coalesce((SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()), false), \
                 version()",
            )
            .await
            .map_err(map_error)?
            .into_iter()
            .find_map(|m| match m {
                SimpleQueryMessage::Row(row) => Some(row),
                _ => None,
            });
        let version = row
            .as_ref()
            .and_then(|r| r.get(0))
            .unwrap_or_default()
            .to_owned();
        let encrypted = row.as_ref().and_then(|r| r.get(1)) == Some("t");
        let (product, version) = product(row.as_ref().and_then(|r| r.get(2)), version);

        Ok(Self {
            canceller: PgCanceller {
                token: client.cancel_token(),
                tls,
            },
            client: Arc::new(client),
            server: ServerInfo {
                product: product.to_owned(),
                version,
                encrypted,
            },
            busy: Arc::new(Mutex::new(())),
            tx: TxTracker::default(),
            next_cursor: AtomicU64::new(1),
        })
    }
}

async fn spawn_connection<T>(config: &tokio_postgres::Config, tls: T) -> Result<Client>
where
    T: MakeTlsConnect<Socket> + Send + 'static,
    T::Stream: Send + 'static,
{
    let (client, connection) = config
        .connect(tls)
        .await
        .map_err(|e| match e.as_db_error() {
            Some(_) => map_error(e),
            None => Error::Connect(error_chain(&e)),
        })?;
    // The connection future drives the socket; it ends when the client is dropped.
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

#[async_trait]
impl Connection for PgConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let effect = classify(sql);
        let guard = Arc::clone(&self.busy).lock_owned().await;
        if self.tx.needs_begin(effect) {
            self.client
                .batch_execute("BEGIN")
                .await
                .map_err(map_error)?;
            self.tx.set_open(true);
        }
        let result = self.execute_locked(sql, options, guard).await;
        self.tx.after(effect, false);
        result
    }

    /// All the statements in one simple query: one round trip for all of them, where
    /// [`execute`](Self::execute) takes two per statement (prepare, then run).
    async fn execute_all(&self, statements: &[&str]) -> Executed {
        // An empty query would still open a transaction in manual-commit mode.
        if statements.is_empty() || statements.iter().any(|s| classify(s) != TxEffect::None) {
            return execute_each(self, statements).await;
        }
        let _guard = self.busy.lock().await;
        if self.tx.needs_begin(TxEffect::None) {
            if let Err(e) = self.client.batch_execute("BEGIN").await {
                return Executed {
                    rows: Vec::new(),
                    error: Some(map_error(e)),
                };
            }
            self.tx.set_open(true);
        }
        let mut rows = Vec::with_capacity(statements.len());
        let result = self.simple_counts(&statements.join(";\n"), &mut rows).await;
        self.tx.after(TxEffect::None, false);
        let error = match result {
            Err(e) => Some(map_error(e)),
            // A statement the server split differently would shift every count after it.
            Ok(()) if rows.len() != statements.len() => Some(Error::Unsupported(format!(
                "the server ran {} statements instead of {}",
                rows.len(),
                statements.len()
            ))),
            Ok(()) => None,
        };
        Executed { rows, error }
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(self.canceller.clone())
    }

    fn server_info(&self) -> &ServerInfo {
        &self.server
    }

    fn is_closed(&self) -> bool {
        self.client.is_closed()
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
        if is_cockroach(self) {
            return Err(Error::Unsupported("CockroachDB query plans".to_owned()));
        }
        explain::explain(self, sql, analyze).await
    }

    async fn object_ddl(&self, object: &platypus_core::ObjectRef<'_>) -> Result<String> {
        ddl::object_ddl(self, object).await
    }

    async fn object_properties(
        &self,
        object: &platypus_core::ObjectRef<'_>,
    ) -> Result<Vec<platypus_core::Property>> {
        ddl::object_properties(self, object).await
    }

    async fn databases(&self) -> Result<Vec<String>> {
        ddl::databases(self).await
    }

    async fn search_metadata(&self, needle: &str) -> Result<Vec<platypus_core::MetadataMatch>> {
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
            // CockroachDB's plans are neither PostgreSQL's JSON nor `EXPLAIN ANALYZE`'s.
            explain: !is_cockroach(self),
            object_ddl: true,
            metadata_search: true,
            multiple_databases: true,
        }
    }
}

impl PgConnection {
    /// Runs `sql` as a simple query, adding the rows each of its statements changed to
    /// `rows`; on an error, `rows` holds the counts of the statements before it.
    async fn simple_counts(
        &self,
        sql: &str,
        rows: &mut Vec<u64>,
    ) -> std::result::Result<(), tokio_postgres::Error> {
        let stream = self.client.simple_query_raw(sql).await?;
        futures::pin_mut!(stream);
        while let Some(message) = stream.next().await.transpose()? {
            if let SimpleQueryMessage::CommandComplete(n) = message {
                rows.push(n);
            }
        }
        Ok(())
    }

    async fn end_transaction(&self, sql: &str) -> Result<()> {
        let _guard = self.busy.lock().await;
        if !self.tx.state().open {
            return Ok(());
        }
        let result = self.client.batch_execute(sql).await.map_err(map_error);
        // Even a failed COMMIT ends the transaction in PostgreSQL.
        self.tx.set_open(false);
        result
    }

    async fn execute_locked(
        &self,
        sql: &str,
        options: &ExecOptions,
        guard: OwnedMutexGuard<()>,
    ) -> Result<ExecResult> {
        let open = self.tx.state().open;
        if (open || options.paged) && cursor::eligible(sql) {
            let name = format!(
                "platypus_cursor_{}",
                self.next_cursor.fetch_add(1, Ordering::Relaxed)
            );
            return cursor::open(Arc::clone(&self.client), &name, sql, options, guard, !open).await;
        }
        let statement = self.client.prepare(sql).await.map_err(map_error)?;

        if statement.columns().is_empty() {
            let rows = self
                .client
                .execute_raw(&statement, no_params())
                .await
                .map_err(map_error)?;
            return Ok(ExecResult::Affected { rows });
        }

        let (fields, builders): (Vec<_>, Vec<_>) = statement
            .columns()
            .iter()
            .map(column_builder)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip();
        let schema = Arc::new(Schema::new(fields));
        let rows = self
            .client
            .query_raw(&statement, no_params())
            .await
            .map_err(map_error)?;

        let state = BatchState {
            rows: Some(Box::pin(rows)),
            builders,
            schema: Arc::clone(&schema),
            batch_size: options.batch_size.max(1),
            canceller: self.canceller.clone(),
            guard: Some(guard),
        };
        let stream = futures::stream::try_unfold(state, next_batch);
        Ok(ExecResult::Rows(RecordBatchStream::new(
            schema,
            stream.boxed(),
        )))
    }
}

fn no_params() -> std::iter::Empty<&'static (dyn ToSql + Sync)> {
    std::iter::empty()
}

struct BatchState {
    /// `None` once the server has sent the whole result (or an error).
    rows: Option<Pin<Box<RowStream>>>,
    builders: Vec<ColumnBuilder>,
    schema: Arc<Schema>,
    batch_size: usize,
    canceller: PgCanceller,
    guard: Option<OwnedMutexGuard<()>>,
}

impl BatchState {
    fn finish(&mut self) {
        self.rows = None;
        self.guard = None;
    }
}

/// Accepts any column type and hands back the raw wire bytes, decoded by [`ColumnBuilder`].
struct Raw<'a>(&'a [u8]);

impl<'a> FromSql<'a> for Raw<'a> {
    fn from_sql(
        _: &Type,
        raw: &'a [u8],
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Raw(raw))
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

/// Decodes one row into the column builders.
pub(crate) fn append_row(
    builders: &mut [ColumnBuilder],
    schema: &Schema,
    row: &tokio_postgres::Row,
) -> Result<()> {
    for (i, builder) in builders.iter_mut().enumerate() {
        let value = row.try_get::<_, Option<Raw<'_>>>(i).map_err(map_error)?;
        builder.append(value.map(|r| r.0)).map_err(|e| {
            let field = schema.field(i);
            Error::Decode {
                column: field.name().clone(),
                db_type: field
                    .metadata()
                    .get(platypus_core::META_DB_TYPE)
                    .cloned()
                    .unwrap_or_default(),
                message: e.to_string(),
            }
        })?;
    }
    Ok(())
}

async fn next_batch(mut state: BatchState) -> Result<Option<(RecordBatch, BatchState)>> {
    let Some(rows) = state.rows.as_mut() else {
        return Ok(None);
    };
    let mut len = 0;
    while len < state.batch_size {
        let row = match rows.next().await {
            Some(Ok(row)) => row,
            Some(Err(e)) => {
                state.finish();
                return Err(map_error(e));
            }
            None => {
                state.finish();
                break;
            }
        };
        append_row(&mut state.builders, &state.schema, &row)?;
        len += 1;
    }
    if len == 0 {
        return Ok(None);
    }
    let columns = state
        .builders
        .iter_mut()
        .map(ColumnBuilder::finish)
        .collect();
    let batch = RecordBatch::try_new(Arc::clone(&state.schema), columns)?;
    Ok(Some((batch, state)))
}

/// How long an abandoned result may drain on its own before it is cancelled.
const DRAIN_BEFORE_CANCEL: Duration = Duration::from_millis(100);

impl Drop for BatchState {
    /// An abandoned result is drained in the background, and cancelled server-side only
    /// if it is still running after a moment; the connection stays busy until it is done.
    ///
    /// A cancel request reaches the backend as a signal, asynchronously: one sent for a
    /// query that had already finished could land on the next statement and cancel it. Most
    /// abandoned results (a paused page of rows) have finished, so they are only drained.
    fn drop(&mut self) {
        let (Some(mut rows), Some(guard)) = (self.rows.take(), self.guard.take()) else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let canceller = self.canceller.clone();
        runtime.spawn(async move {
            let drained = tokio::time::timeout(DRAIN_BEFORE_CANCEL, async {
                while rows.next().await.is_some() {}
            })
            .await;
            if drained.is_err() {
                let _ = canceller.cancel().await;
                while rows.next().await.is_some() {}
            }
            drop(guard);
        });
    }
}

/// Cancel requests open a new connection, so they need the same TLS setup as the session.
#[derive(Clone)]
struct PgCanceller {
    token: CancelToken,
    tls: Option<MakeRustlsConnect>,
}

impl std::fmt::Debug for PgCanceller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgCanceller")
            .field("tls", &self.tls.is_some())
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl Canceller for PgCanceller {
    async fn cancel(&self) -> Result<()> {
        let result = match &self.tls {
            Some(tls) => self.token.cancel_query(tls.clone()).await,
            None => self.token.cancel_query(NoTls).await,
        };
        result.map_err(|e| Error::Connect(format!("cancel request failed: {e}")))
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn map_error(e: tokio_postgres::Error) -> Error {
    if let Some(db) = e.as_db_error() {
        if *db.code() == SqlState::QUERY_CANCELED {
            return Error::Cancelled;
        }
        return Error::Database(DbError {
            code: Some(db.code().code().to_owned()),
            message: db.message().to_owned(),
            detail: db.detail().map(str::to_owned),
            hint: db.hint().map(str::to_owned),
            position: match db.position() {
                Some(ErrorPosition::Original(p)) => Some(*p),
                _ => None,
            },
        });
    }
    if e.is_closed() {
        return Error::Closed;
    }
    Error::Other(error_chain(&e))
}

/// `tokio-postgres` errors keep the useful part (e.g. the TLS failure) in their sources.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut message = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}
