use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use async_trait::async_trait;
use futures::Stream;
use futures::stream::BoxStream;

use crate::{
    ConnectionConfig, DriverKind, ObjectMeta, Result, SchemaMeta, TableDetails, TransactionState,
};

#[async_trait]
pub trait Driver: Send + Sync + fmt::Debug {
    fn kind(&self) -> DriverKind;

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>>;
}

/// An open session with a database server.
///
/// `execute` may be called again while a previous [`RecordBatchStream`] is still alive;
/// drivers queue the statements. Dropping an unfinished stream cancels its query.
#[async_trait]
pub trait Connection: Send + Sync + fmt::Debug {
    /// Executes a single SQL statement.
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult>;

    /// A handle that cancels the statement currently running on this connection.
    /// It is independent of the connection, so it can be used from another task.
    fn cancel_handle(&self) -> CancelHandle;

    fn server_info(&self) -> &ServerInfo;

    fn is_closed(&self) -> bool;

    /// Schemas (MySQL databases, SQLite attached databases) visible to the session.
    async fn schemas(&self) -> Result<Vec<SchemaMeta>>;

    /// Tables, views, sequences and routines in `schema`, sorted by name.
    async fn objects(&self, schema: &str) -> Result<Vec<ObjectMeta>>;

    /// Columns, indexes and foreign keys of a table or view.
    async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails>;

    fn transaction(&self) -> TransactionState;

    /// Switches between autocommit and manual commit; fails while a transaction is open.
    async fn set_autocommit(&self, on: bool) -> Result<()>;

    /// Commits the open transaction, if any.
    async fn commit(&self) -> Result<()>;

    /// Rolls back the open transaction, if any.
    async fn rollback(&self) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ServerInfo {
    /// Product name, e.g. `PostgreSQL`.
    pub product: String,
    /// Server version string as reported by the server.
    pub version: String,
    /// Whether the session is TLS-encrypted.
    pub encrypted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOptions {
    /// Maximum number of rows per [`RecordBatch`].
    pub batch_size: usize,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self { batch_size: 1024 }
    }
}

pub enum ExecResult {
    /// A statement that returns rows (`SELECT`, `RETURNING`, `SHOW`, ...).
    Rows(RecordBatchStream),
    /// A statement that does not return rows, with the number of rows it affected.
    Affected { rows: u64 },
}

impl fmt::Debug for ExecResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rows(stream) => f.debug_tuple("Rows").field(stream).finish(),
            Self::Affected { rows } => f.debug_struct("Affected").field("rows", rows).finish(),
        }
    }
}

/// A stream of Arrow record batches sharing one schema, known before the first batch arrives.
pub struct RecordBatchStream {
    schema: SchemaRef,
    inner: BoxStream<'static, Result<RecordBatch>>,
}

impl RecordBatchStream {
    #[must_use]
    pub fn new(schema: SchemaRef, inner: BoxStream<'static, Result<RecordBatch>>) -> Self {
        Self { schema, inner }
    }

    #[must_use]
    pub fn schema(&self) -> SchemaRef {
        Arc::clone(&self.schema)
    }
}

impl Stream for RecordBatchStream {
    type Item = Result<RecordBatch>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

impl fmt::Debug for RecordBatchStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecordBatchStream")
            .field("schema", &self.schema)
            .finish_non_exhaustive()
    }
}

#[async_trait]
pub trait Canceller: Send + Sync {
    async fn cancel(&self) -> Result<()>;
}

/// Cheaply clonable handle that cancels the query running on a connection.
#[derive(Clone)]
pub struct CancelHandle(Arc<dyn Canceller>);

impl CancelHandle {
    pub fn new(canceller: impl Canceller + 'static) -> Self {
        Self(Arc::new(canceller))
    }

    /// Asks the server to cancel the running statement. Succeeds even if nothing was running.
    pub async fn cancel(&self) -> Result<()> {
        self.0.cancel().await
    }
}

impl fmt::Debug for CancelHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CancelHandle")
    }
}
