//! libSQL driver: `sqld` servers and Turso, over the Hrana protocol on HTTP.
//!
//! The SQL is SQLite's, so the metadata queries and the typing of results are the SQLite
//! driver's. Statements run one at a time on a server-side stream, kept between requests
//! by its baton so that transactions and temporary tables last. A stream the server let
//! expire is replaced outside a transaction; inside one, the connection reports itself
//! closed. Results arrive whole, and HTTP has no way to stop a running statement.

mod hrana;
mod session;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use async_trait::async_trait;
use futures::StreamExt;
use platypus_core::{
    CancelHandle, Canceller, Capabilities, CatalogColumn, CatalogForeignKey, Connection,
    ConnectionConfig, Driver, DriverKind, Error, ExecOptions, ExecResult, Explain, MetadataMatch,
    ObjectMeta, ObjectRef, RecordBatchStream, ReferencingKey, Result, SchemaMeta, ServerInfo,
    TableDetails, TransactionState, TxEffect, classify, open_transaction_error, unsupported,
};
use platypus_driver_sqlite::columns::{self, ColumnBuilder, Kind, Owned};
use platypus_driver_sqlite::meta;

use crate::hrana::StmtResult;
use crate::session::Session;

#[derive(Debug, Default, Clone, Copy)]
pub struct LibsqlDriver;

#[async_trait]
impl Driver for LibsqlDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Libsql
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let conn: Box<dyn Connection> = Box::new(LibsqlConnection::connect(config).await?);
        platypus_core::run_init(&*conn, &config.init, self.kind()).await?;
        Ok(conn)
    }
}

#[derive(Debug)]
pub struct LibsqlConnection {
    /// Locked for each statement: they run in order, on one stream.
    session: tokio::sync::Mutex<Session>,
    server: ServerInfo,
    autocommit: AtomicBool,
    /// A transaction is open on the stream, as the server reported after the last statement.
    in_transaction: Arc<AtomicBool>,
    /// The stream of an open transaction was lost.
    closed: Arc<AtomicBool>,
}

impl LibsqlConnection {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self> {
        if config.driver != DriverKind::Libsql {
            return Err(Error::Config(format!(
                "{:?} is not a libSQL config",
                config.driver
            )));
        }
        let (session, encrypted, version) = Session::open(config).await?;
        Ok(Self {
            in_transaction: Arc::clone(&session.in_transaction),
            closed: Arc::clone(&session.closed),
            session: tokio::sync::Mutex::new(session),
            server: ServerInfo {
                product: "libSQL".to_owned(),
                version,
                encrypted,
            },
            autocommit: AtomicBool::new(true),
        })
    }

    async fn run(&self, sql: &str) -> Result<StmtResult> {
        self.session.lock().await.execute(sql).await
    }
}

#[async_trait]
impl Connection for LibsqlConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let state = self.transaction();
        // DDL too: it is transactional in SQLite.
        if !state.autocommit
            && !state.open
            && matches!(classify(sql), TxEffect::None | TxEffect::ImplicitCommit)
        {
            self.run("BEGIN").await?;
        }
        let result = self.run(sql).await?;
        if result.cols.is_empty() {
            return Ok(ExecResult::Affected {
                rows: result.affected_row_count,
            });
        }
        let batch_size = options.batch_size.max(1);
        let (schema, kinds, rows) = typed_rows(result, batch_size)?;
        Ok(ExecResult::Rows(RecordBatchStream::new(
            Arc::clone(&schema),
            batches(schema, kinds, rows, batch_size).boxed(),
        )))
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(NoCancel)
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

    /// SQLite plans have no costs, and no form that runs the statement: `analyze` is ignored.
    async fn explain(&self, sql: &str, _analyze: bool) -> Result<Explain> {
        meta::explain(self, sql).await
    }

    async fn object_ddl(&self, object: &ObjectRef<'_>) -> Result<String> {
        meta::object_ddl(self, object).await
    }

    async fn search_metadata(&self, needle: &str) -> Result<Vec<MetadataMatch>> {
        meta::search_metadata(self, needle).await
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
            self.run("COMMIT").await?;
        }
        Ok(())
    }

    async fn rollback(&self) -> Result<()> {
        if self.transaction().open {
            self.run("ROLLBACK").await?;
        }
        Ok(())
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: true,
            explain: true,
            object_ddl: true,
            metadata_search: true,
            ..Capabilities::default()
        }
    }
}

/// The schema, column kinds and rows of a result; the kinds are inferred from the first
/// batch, as the SQLite driver does.
fn typed_rows(
    result: StmtResult,
    batch_size: usize,
) -> Result<(SchemaRef, Vec<Kind>, Vec<Vec<Owned>>)> {
    let columns: Vec<(String, Option<String>)> = result
        .cols
        .into_iter()
        .map(|c| (c.name.unwrap_or_default(), c.decltype))
        .collect();
    let rows = result
        .rows
        .into_iter()
        .map(|row| row.into_iter().map(hrana::Value::into_owned).collect())
        .collect::<Result<Vec<Vec<Owned>>>>()?;
    let (schema, kinds) = columns::schema(&columns, &rows[..rows.len().min(batch_size)]);
    Ok((schema, kinds, rows))
}

/// The rows as record batches of at most `batch_size` rows, built as they are read.
fn batches(
    schema: SchemaRef,
    kinds: Vec<Kind>,
    rows: Vec<Vec<Owned>>,
    batch_size: usize,
) -> impl futures::Stream<Item = Result<RecordBatch>> {
    let mut rows = rows.into_iter().peekable();
    let mut failed = false;
    futures::stream::iter(std::iter::from_fn(move || {
        if failed {
            return None;
        }
        rows.peek()?;
        let mut builders: Vec<ColumnBuilder> = kinds
            .iter()
            .map(|k| ColumnBuilder::new(*k, batch_size))
            .collect();
        for row in rows.by_ref().take(batch_size) {
            for (i, value) in row.iter().enumerate() {
                if let Err(m) = builders[i].append(value.value()) {
                    failed = true;
                    return Some(Err(m.error(schema.field(i))));
                }
            }
        }
        let arrays = builders.iter_mut().map(ColumnBuilder::finish).collect();
        Some(RecordBatch::try_new(Arc::clone(&schema), arrays).map_err(Error::from))
    }))
}

/// HTTP cannot stop a statement the server is running.
struct NoCancel;

#[async_trait]
impl Canceller for NoCancel {
    async fn cancel(&self) -> Result<()> {
        Err(unsupported("cancelling libSQL statements"))
    }
}

#[cfg(test)]
mod tests {
    use futures::TryStreamExt;

    use super::*;
    use crate::hrana::Col;

    fn result(json: &str) -> StmtResult {
        serde_json::from_str(json).unwrap()
    }

    #[tokio::test]
    async fn splits_rows_into_typed_batches() {
        let result = result(
            r#"{"cols":[{"name":"id","decltype":"INTEGER"},{"name":"v","decltype":null}],
                "rows":[[{"type":"integer","value":"1"},{"type":"text","value":"a"}],
                        [{"type":"integer","value":"2"},{"type":"null"}],
                        [{"type":"integer","value":"3"},{"type":"text","value":"c"}]],
                "affected_row_count":0}"#,
        );
        let (schema, kinds, rows) = typed_rows(result, 2).unwrap();
        assert_eq!(kinds, [Kind::Int, Kind::Text]);
        let out: Vec<RecordBatch> = batches(schema, kinds, rows, 2).try_collect().await.unwrap();
        assert_eq!(
            out.iter().map(RecordBatch::num_rows).collect::<Vec<_>>(),
            [2, 1]
        );
    }

    #[tokio::test]
    async fn fails_on_values_that_do_not_fit_the_first_rows() {
        let result = result(
            r#"{"cols":[{"name":"x","decltype":null}],
                "rows":[[{"type":"integer","value":"1"}],[{"type":"blob","base64":"AQ=="}]],
                "affected_row_count":0}"#,
        );
        let (schema, kinds, rows) = typed_rows(result, 1).unwrap();
        let out: Vec<Result<RecordBatch>> = batches(schema, kinds, rows, 1).collect().await;
        assert_eq!(out.len(), 2);
        assert!(matches!(out[1], Err(Error::Decode { .. })), "{out:?}");
    }

    #[test]
    fn names_unnamed_columns() {
        let result = StmtResult {
            cols: vec![Col {
                name: None,
                decltype: None,
            }],
            rows: Vec::new(),
            affected_row_count: 0,
        };
        let (schema, _, rows) = typed_rows(result, 10).unwrap();
        assert_eq!(schema.field(0).name(), "");
        assert_eq!(rows.len(), 0);
    }
}
