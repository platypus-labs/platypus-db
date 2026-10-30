use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use async_trait::async_trait;
use futures::Stream;
use futures::future::BoxFuture;
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

    /// Runs `statements`, which return no rows and control no transaction, in order, up to
    /// the first that fails or changes a number of rows other than its entry in `expected`
    /// (where it has one). Drivers that can send them together, saving a round trip per
    /// statement, do, and may have run some past that one; the default runs them one at a
    /// time with [`execute`](Self::execute).
    async fn execute_all(&self, statements: &[&str], expected: &[u64]) -> Executed {
        execute_each(self, statements, expected).await
    }

    /// A handle that cancels the statement currently running on this connection.
    /// It is independent of the connection, so it can be used from another task.
    fn cancel_handle(&self) -> CancelHandle;

    fn server_info(&self) -> &ServerInfo;

    fn is_closed(&self) -> bool;

    /// The messages the server sent with the statements run since the last call: PostgreSQL
    /// notices, Transact-SQL `PRINT` and low-severity `RAISERROR`, MySQL warnings. Drivers
    /// without any answer none.
    fn take_messages(&self) -> Vec<String> {
        Vec::new()
    }

    /// Whether the server still holds the session, asked with a light round trip by the
    /// drivers that learn of a dropped session only from the next statement (MySQL, SQL
    /// Server); the others answer [`is_closed`](Self::is_closed). Run before a statement
    /// on a session that sat idle, so that a session the server closed is reopened before
    /// the statement rather than failing it. It never opens a transaction, and a session
    /// busy with a statement counts as alive.
    async fn check_alive(&self) -> bool {
        !self.is_closed()
    }

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

    /// The `CREATE` statements of several objects, each with its own result, in their order.
    /// One at a time by default; a driver that reads the catalog of many objects in a few
    /// queries overrides it, which saves a round trip per object on a remote database.
    async fn objects_ddl(&self, objects: &[ObjectRef<'_>]) -> Vec<Result<String>> {
        let mut out = Vec::with_capacity(objects.len());
        for object in objects {
            out.push(self.object_ddl(object).await);
        }
        out
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

    /// The sessions of the server, the one asking included (marked
    /// [`current`](crate::ServerSession::current)), with the statements they run; those of
    /// other users only where the session may see them. Optional: see
    /// [`Capabilities::server_sessions`].
    async fn server_sessions(&self) -> Result<Vec<crate::ServerSession>> {
        Err(unsupported("server sessions"))
    }

    /// The locks the sessions of the server hold or wait for, those of the session asking
    /// left out. Optional: see [`Capabilities::server_sessions`].
    async fn server_locks(&self) -> Result<Vec<crate::ServerLock>> {
        Err(unsupported("server locks"))
    }

    /// Writes `rows` into a table with the database's own bulk loading (PostgreSQL's `COPY`,
    /// MySQL's `LOAD DATA LOCAL`), in the session's transaction; the rows written. Optional:
    /// see [`Capabilities::bulk_load`]. A server that refuses it (MySQL without
    /// `local_infile`), or rows it cannot load this way (SQL Server's bulk copy into a
    /// `money` column), answers [`Error::Unsupported`], and nothing was written. No rows only
    /// check the table.
    async fn bulk_load(&self, rows: &BulkRows<'_>) -> Result<u64> {
        let _ = rows;
        Err(unsupported("bulk loading"))
    }

    /// Cancels the statement of session `id` of the server, or closes the session.
    /// Optional: see [`Capabilities::server_sessions`] and
    /// [`Capabilities::cancel_server_queries`].
    async fn end_server_session(&self, id: &str, how: crate::EndSession) -> Result<()> {
        let _ = (id, how);
        Err(unsupported("ending server sessions"))
    }

    /// The statements that took the server most time, at most
    /// [`TOP_QUERIES`](crate::TOP_QUERIES), the costliest first. Optional: see
    /// [`Capabilities::server_statistics`].
    async fn top_queries(&self) -> Result<Vec<crate::ServerQuery>> {
        Err(unsupported("query statistics"))
    }

    /// The tables and indexes of the user schemas of the database, with how they have been
    /// used. Optional: see [`Capabilities::server_statistics`].
    async fn object_statistics(&self) -> Result<Vec<crate::ObjectStatistics>> {
        Err(unsupported("table statistics"))
    }

    /// The configuration parameters of the server, by name, with their defaults where the
    /// server tells them. Optional: see [`Capabilities::server_parameters`].
    async fn server_parameters(&self) -> Result<Vec<crate::ServerParameter>> {
        Err(unsupported("server parameters"))
    }

    /// A snapshot of the server's load: connections, counters, sizes and replication. What
    /// the server does not tell, or the session may not read, is missing in it with the
    /// reason. Optional: see [`Capabilities::server_metrics`].
    async fn server_metrics(&self) -> Result<crate::ServerMetrics> {
        Err(unsupported("server metrics"))
    }

    /// Runs `operation` on table `schema.table`; returns what the server said of it, a line
    /// each. Optional: see [`Capabilities::maintenance`].
    async fn maintain(
        &self,
        schema: &str,
        table: &str,
        operation: crate::Maintenance,
    ) -> Result<Vec<String>> {
        let _ = (schema, table, operation);
        Err(unsupported("table maintenance"))
    }

    /// The users and roles of the server, sorted by name, with the roles each belongs to.
    /// Optional: see [`Capabilities::security`].
    async fn principals(&self) -> Result<Vec<crate::Principal>> {
        Err(unsupported("users and roles"))
    }

    /// The privileges given to the user or role `name` (with `host`, for a MySQL account)
    /// itself: not those it has through the roles it belongs to. Optional: see
    /// [`Capabilities::security`].
    async fn principal_privileges(
        &self,
        name: &str,
        host: Option<&str>,
    ) -> Result<Vec<crate::PrivilegeGrant>> {
        let _ = (name, host);
        Err(unsupported("privileges"))
    }

    /// Objects of the user schemas whose name, column names or source contain `needle`,
    /// ignoring case; at most [`SEARCH_LIMIT`](crate::SEARCH_LIMIT) of them.
    /// Optional: see [`Capabilities::metadata_search`].
    async fn search_metadata(&self, needle: &str) -> Result<Vec<MetadataMatch>> {
        let _ = needle;
        Err(unsupported("metadata search"))
    }
}

/// What [`Connection::execute_all`] ran.
#[derive(Debug, Default)]
pub struct Executed {
    /// The rows each statement that ran changed, in order.
    pub rows: Vec<u64>,
    /// Why the statement after those stopped the run.
    pub error: Option<crate::Error>,
}

/// [`Connection::execute_all`] one statement at a time, for drivers that cannot send
/// several together: it stops at the first that fails or changes other than its
/// `expected` rows.
pub async fn execute_each<C: Connection + ?Sized>(
    conn: &C,
    statements: &[&str],
    expected: &[u64],
) -> Executed {
    let mut rows = Vec::with_capacity(statements.len());
    for (i, sql) in statements.iter().enumerate() {
        let n = match conn.execute(sql, &ExecOptions::default()).await {
            Ok(ExecResult::Affected { rows: n }) => n,
            // Dropping the stream ends the statement.
            Ok(ExecResult::Rows(_)) => 0,
            Err(error) => {
                return Executed {
                    rows,
                    error: Some(error),
                };
            }
        };
        rows.push(n);
        if expected.get(i).is_some_and(|&e| e != n) {
            break;
        }
    }
    Executed { rows, error: None }
}

/// Rows for [`Connection::bulk_load`], as text: each value as the database reads it from
/// text input, `None` for NULL.
#[derive(Debug, Clone, Copy)]
pub struct BulkRows<'a> {
    /// The table, qualified and quoted as its database writes it.
    pub table: &'a str,
    /// Its columns the values go to, quoted.
    pub columns: &'a [String],
    /// The columns whose values are the hex text of bytes (`deadbeef`).
    pub hex: &'a [bool],
    pub rows: &'a [Vec<Option<String>>],
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

/// The next result of a statement that returns several (a Transact-SQL batch with two
/// `SELECT`s, a MySQL `CALL`): it resolves once the result before it has been read to its
/// end, to `None` when there is no other. Dropping it leaves the rest of the statement to run
/// to its end, its results unread.
pub type NextResult = BoxFuture<'static, Result<Option<ExecResult>>>;

/// A stream of Arrow record batches sharing one schema, known before the first batch arrives.
pub struct RecordBatchStream {
    schema: SchemaRef,
    inner: BoxStream<'static, Result<RecordBatch>>,
    next: Option<NextResult>,
}

impl RecordBatchStream {
    #[must_use]
    pub fn new(schema: SchemaRef, inner: BoxStream<'static, Result<RecordBatch>>) -> Self {
        Self {
            schema,
            inner,
            next: None,
        }
    }

    /// The same stream, followed by the statement's next result.
    #[must_use]
    pub fn with_next(mut self, next: Option<NextResult>) -> Self {
        self.next = next;
        self
    }

    /// Takes what follows this result (see [`NextResult`]), to await once the stream ended.
    pub fn take_next(&mut self) -> Option<NextResult> {
        self.next.take()
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
