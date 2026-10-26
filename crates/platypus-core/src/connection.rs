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
    Capabilities, CatalogColumn, CatalogForeignKey, ConnectionConfig, DriverKind, Explain,
    MetadataMatch, ObjectMeta, ObjectRef, Property, ReferencingKey, Result, SchemaMeta,
    TableDetails, TransactionState, unsupported,
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

    /// The foreign keys of other tables that point at `schema.table`.
    async fn referencing_keys(&self, schema: &str, table: &str) -> Result<Vec<ReferencingKey>>;

    /// Every column of every table and view in the user schemas, for autocompletion.
    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>>;

    /// Every foreign key between the tables of the user schemas, for completing JOIN
    /// conditions. Drivers without it complete no JOINs.
    async fn catalog_foreign_keys(&self) -> Result<Vec<CatalogForeignKey>> {
        Ok(Vec::new())
    }

    fn transaction(&self) -> TransactionState;

    /// Switches between autocommit and manual commit; fails while a transaction is open.
    async fn set_autocommit(&self, on: bool) -> Result<()>;

    /// Commits the open transaction, if any.
    async fn commit(&self) -> Result<()>;

    /// Rolls back the open transaction, if any.
    async fn rollback(&self) -> Result<()>;

    /// Which optional methods below the driver implements.
    fn capabilities(&self) -> Capabilities;

    /// The plan of `sql`; with `analyze` the statement is executed to get real rows and
    /// timings, and what it changes is rolled back (see [`explain_document`](crate::explain_document)).
    /// Optional: see [`Capabilities::explain`].
    async fn explain(&self, sql: &str, analyze: bool) -> Result<Explain> {
        let _ = (sql, analyze);
        Err(unsupported("explain"))
    }

    /// The `CREATE` statement of an object. Optional: see [`Capabilities::object_ddl`].
    async fn object_ddl(&self, object: &ObjectRef<'_>) -> Result<String> {
        let _ = object;
        Err(unsupported("object DDL"))
    }

    /// Descriptive properties of an object (owner, size, row estimate, ...), in display
    /// order. Empty when the driver has none to show.
    async fn object_properties(&self, object: &ObjectRef<'_>) -> Result<Vec<Property>> {
        let _ = object;
        Ok(Vec::new())
    }

    /// The comment of table or view `name`, from the object list unless the driver has a
    /// cheaper query.
    async fn object_comment(&self, schema: &str, name: &str) -> Result<Option<String>> {
        Ok(self
            .objects(schema)
            .await?
            .into_iter()
            .find(|o| o.name == name)
            .and_then(|o| o.comment))
    }

    /// The databases of the server this session could switch to, sorted by name.
    /// Optional: see [`Capabilities::multiple_databases`].
    async fn databases(&self) -> Result<Vec<String>> {
        Err(unsupported("listing databases"))
    }

    /// Objects of the user schemas whose name, column names or source contain `needle`,
    /// ignoring case; at most [`SEARCH_LIMIT`](crate::SEARCH_LIMIT) of them.
    /// Optional: see [`Capabilities::metadata_search`].
    async fn search_metadata(&self, needle: &str) -> Result<Vec<MetadataMatch>> {
        let _ = needle;
        Err(unsupported("metadata search"))
    }
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
    /// The reader takes a page at a time and may leave the rest (a result grid): the driver
    /// should fetch `batch_size` rows per request, so the server never runs ahead and an
    /// abandoned result ends at once instead of being drained.
    pub paged: bool,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self {
            batch_size: 1024,
            paged: false,
        }
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
