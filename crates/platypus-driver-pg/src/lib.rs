//! PostgreSQL driver built on `tokio-postgres`.
//!
//! Rows are fetched in the binary protocol and decoded straight into Arrow builders, one
//! batch at a time: the next batch is only read from the socket when the consumer polls for
//! it, so memory stays flat regardless of result size.
//!
//! TLS is not supported yet: `sslmode=disable` and `prefer` connect in plaintext,
//! stricter modes are rejected.

#[doc(hidden)]
pub mod columns;
#[doc(hidden)]
pub mod decode;

use std::pin::Pin;
use std::sync::Arc;

use arrow_array::RecordBatch;
use arrow_schema::Schema;
use async_trait::async_trait;
use futures::StreamExt;
use platypus_core::{
    CancelHandle, Canceller, Connection, ConnectionConfig, DbError, Driver, DriverKind, Error,
    ExecOptions, ExecResult, RecordBatchStream, Result, ServerInfo, SslMode,
};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio_postgres::error::{ErrorPosition, SqlState};
use tokio_postgres::types::{FromSql, ToSql, Type};
use tokio_postgres::{CancelToken, Client, NoTls, RowStream, SimpleQueryMessage};

use crate::columns::{ColumnBuilder, column_builder};

#[derive(Debug, Default, Clone, Copy)]
pub struct PgDriver;

#[async_trait]
impl Driver for PgDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Postgres
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        Ok(Box::new(PgConnection::connect(config).await?))
    }
}

#[derive(Debug)]
pub struct PgConnection {
    client: Client,
    server: ServerInfo,
    /// Held for the lifetime of a statement, including its result stream, so a new
    /// statement never starts while an abandoned one is still being cancelled.
    busy: Arc<Mutex<()>>,
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
        pg.host(&config.host)
            .port(config.port.unwrap_or(5432))
            .dbname(&config.database)
            .user(&config.user);
        if let Some(password) = &config.password {
            pg.password(password.expose());
        }
        if let Some(name) = &config.application_name {
            pg.application_name(name);
        }
        if let Some(timeout) = config.connect_timeout {
            pg.connect_timeout(timeout);
        }
        pg.ssl_mode(match config.ssl_mode {
            SslMode::Disable => tokio_postgres::config::SslMode::Disable,
            SslMode::Prefer => tokio_postgres::config::SslMode::Prefer,
            mode => {
                return Err(Error::Unsupported(format!(
                    "sslmode {mode:?} (TLS lands in M2)"
                )));
            }
        });

        let (client, connection) = pg.connect(NoTls).await.map_err(|e| match e.as_db_error() {
            Some(_) => map_error(e),
            None => Error::Connect(e.to_string()),
        })?;
        // The connection future drives the socket; it ends when the client is dropped.
        tokio::spawn(async move {
            let _ = connection.await;
        });

        let version = client
            .simple_query("SHOW server_version")
            .await
            .map_err(map_error)?
            .into_iter()
            .find_map(|m| match m {
                SimpleQueryMessage::Row(row) => row.get(0).map(str::to_owned),
                _ => None,
            })
            .unwrap_or_default();

        Ok(Self {
            client,
            server: ServerInfo {
                product: "PostgreSQL".to_owned(),
                version,
            },
            busy: Arc::new(Mutex::new(())),
        })
    }
}

#[async_trait]
impl Connection for PgConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let guard = Arc::clone(&self.busy).lock_owned().await;
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
            cancel: self.client.cancel_token(),
            guard: Some(guard),
        };
        let stream = futures::stream::try_unfold(state, next_batch);
        Ok(ExecResult::Rows(RecordBatchStream::new(
            schema,
            stream.boxed(),
        )))
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(PgCanceller(self.client.cancel_token()))
    }

    fn server_info(&self) -> &ServerInfo {
        &self.server
    }

    fn is_closed(&self) -> bool {
        self.client.is_closed()
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
    cancel: CancelToken,
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
        for (i, builder) in state.builders.iter_mut().enumerate() {
            let value = row.try_get::<_, Option<Raw<'_>>>(i).map_err(map_error)?;
            builder.append(value.map(|r| r.0)).map_err(|e| {
                let field = state.schema.field(i);
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

impl Drop for BatchState {
    /// An abandoned result is cancelled server-side and drained in the background; the
    /// connection stays busy until then, so the cancel cannot hit the next statement.
    fn drop(&mut self) {
        let (Some(mut rows), Some(guard)) = (self.rows.take(), self.guard.take()) else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let cancel = self.cancel.clone();
        runtime.spawn(async move {
            let _ = cancel.cancel_query(NoTls).await;
            while rows.next().await.is_some() {}
            drop(guard);
        });
    }
}

struct PgCanceller(CancelToken);

#[async_trait]
impl Canceller for PgCanceller {
    async fn cancel(&self) -> Result<()> {
        self.0
            .cancel_query(NoTls)
            .await
            .map_err(|e| Error::Connect(format!("cancel request failed: {e}")))
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
    Error::Other(e.to_string())
}
