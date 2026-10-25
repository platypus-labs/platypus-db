//! A session on a database through an ODBC driver: the async side of the thread that owns
//! the ODBC connection (`session`).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::StreamExt as _;
use platypus_core::{
    CATALOG_LIMIT, CancelHandle, Canceller, Capabilities, CatalogColumn, Connection,
    ConnectionConfig, DriverKind, Error, ExecOptions, ExecResult, ObjectMeta, RecordBatchStream,
    ReferencingKey, Result, SchemaMeta, ServerInfo, SslMode, TableDetails, TextRowExt as _,
    TransactionState, TxTracker, query_text,
};
use tokio::sync::{Notify, oneshot};

use crate::session::{self, CatalogRequest, CatalogRows, Command, Started};
use crate::{control, meta};

/// The queries that read the server's version and the session's schema, per database.
fn version_sql(kind: DriverKind) -> Option<&'static str> {
    match kind {
        DriverKind::Oracle => {
            Some("SELECT VERSION FROM PRODUCT_COMPONENT_VERSION WHERE PRODUCT LIKE 'Oracle%'")
        }
        DriverKind::Db2 => Some("SELECT SERVICE_LEVEL FROM TABLE (SYSPROC.ENV_GET_INST_INFO()) I"),
        _ => None,
    }
}

fn current_schema_sql(kind: DriverKind) -> Option<&'static str> {
    match kind {
        DriverKind::Oracle => Some("SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM DUAL"),
        DriverKind::Db2 => Some("VALUES CURRENT SCHEMA"),
        _ => None,
    }
}

#[derive(Debug)]
pub struct OdbcConnection {
    commands: Mutex<std_mpsc::Sender<Command>>,
    canceller: OdbcCanceller,
    kind: DriverKind,
    server: ServerInfo,
    tx: TxTracker,
}

impl OdbcConnection {
    pub async fn open(config: &ConnectionConfig) -> Result<Self> {
        let connection_string = crate::connection_string(config)?;
        let timeout = config
            .connect_timeout
            .map(|t| u32::try_from(t.as_secs()).unwrap_or(u32::MAX));
        let (commands, opened) = session::spawn(connection_string, timeout)?;
        let opened = opened.await.map_err(|_| Error::Closed)??;
        let mut conn = Self {
            commands: Mutex::new(commands),
            canceller: OdbcCanceller::default(),
            kind: config.driver,
            server: ServerInfo {
                product: match config.driver {
                    DriverKind::Odbc => opened.dbms.unwrap_or_else(|| "ODBC".to_owned()),
                    named => named.name().to_owned(),
                },
                version: String::new(),
                // Db2 takes TLS from the connection's fields; other drivers keep it in
                // their own settings, which the app does not see.
                encrypted: config.driver == DriverKind::Db2
                    && !matches!(config.ssl_mode, SslMode::Disable | SslMode::Prefer),
            },
            tx: TxTracker::default(),
        };
        if let Some(sql) = version_sql(config.driver) {
            conn.server.version = conn.scalar(sql).await.unwrap_or_default();
        }
        Ok(conn)
    }

    fn send(&self, command: Command) -> Result<()> {
        self.commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .send(command)
            .map_err(|_| Error::Closed)
    }

    /// The first value of a query's first row, as text.
    async fn scalar(&self, sql: &str) -> Option<String> {
        query_text(self, sql).await.ok()?.first()?.opt(0)
    }

    async fn catalog(&self, request: CatalogRequest) -> Result<CatalogRows> {
        let (reply, answer) = oneshot::channel();
        self.send(Command::Catalog { request, reply })?;
        answer.await.map_err(|_| Error::Closed)?
    }

    async fn columns(&self, schema: &str, table: Option<&str>) -> Result<Vec<meta::ColumnRow>> {
        match self
            .catalog(CatalogRequest::Columns {
                schema: schema.to_owned(),
                table: table.map(str::to_owned),
            })
            .await?
        {
            CatalogRows::Columns(rows) => Ok(rows),
            _ => Err(unexpected()),
        }
    }

    async fn foreign_keys(&self, request: CatalogRequest) -> Result<Vec<meta::ForeignKeyRow>> {
        match self.catalog(request).await? {
            CatalogRows::ForeignKeys(rows) => Ok(rows),
            _ => Err(unexpected()),
        }
    }

    async fn transaction_command(
        &self,
        command: impl FnOnce(oneshot::Sender<Result<()>>) -> Command,
    ) -> Result<()> {
        let (reply, answer) = oneshot::channel();
        self.send(command(reply))?;
        answer.await.map_err(|_| Error::Closed)?
    }
}

fn unexpected() -> Error {
    Error::Other("unexpected catalog answer".to_owned())
}

#[async_trait]
impl Connection for OdbcConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let sql = crate::statement_text(self.kind, sql);
        let (reply, started) = oneshot::channel();
        self.send(Command::Execute {
            sql: sql.to_owned(),
            batch_size: options.batch_size.max(1),
            reply,
        })?;
        let state = &self.canceller.state;
        if state.abandoned.load(Ordering::SeqCst) {
            return Err(Error::Closed);
        }
        state.starting.fetch_add(1, Ordering::SeqCst);
        let abandoned = state.abandon.notified();
        let started = tokio::select! {
            started = started => started,
            () = abandoned => {
                state.starting.fetch_sub(1, Ordering::SeqCst);
                return Err(Error::Cancelled);
            }
        };
        state.starting.fetch_sub(1, Ordering::SeqCst);
        // Oracle commits before and after DDL, in either mode.
        self.tx
            .after(control::effect(sql), self.kind == DriverKind::Oracle);
        match started.map_err(|_| Error::Closed)?? {
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

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(self.canceller.clone())
    }

    fn server_info(&self) -> &ServerInfo {
        &self.server
    }

    /// After a cancelled statement that had not started returning: the worker is still
    /// waiting for it, so the app opens another session.
    fn is_closed(&self) -> bool {
        self.canceller.state.abandoned.load(Ordering::SeqCst)
    }

    async fn schemas(&self) -> Result<Vec<SchemaMeta>> {
        let CatalogRows::Schemas(names) = self.catalog(CatalogRequest::Schemas).await? else {
            return Err(unexpected());
        };
        let current = match current_schema_sql(self.kind) {
            Some(sql) => self.scalar(sql).await,
            None => None,
        };
        Ok(meta::schemas(names, current.as_deref(), self.kind))
    }

    async fn objects(&self, schema: &str) -> Result<Vec<ObjectMeta>> {
        let request = CatalogRequest::Tables {
            schema: Some(schema.to_owned()),
        };
        match self.catalog(request).await? {
            CatalogRows::Tables(rows) => Ok(meta::objects(rows)),
            _ => Err(unexpected()),
        }
    }

    async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails> {
        let columns = self.columns(schema, Some(table)).await?;
        let names = || (schema.to_owned(), table.to_owned());
        let (s, t) = names();
        let CatalogRows::Keys(primary) = self
            .catalog(CatalogRequest::PrimaryKey {
                schema: s,
                table: t,
            })
            .await?
        else {
            return Err(unexpected());
        };
        let (s, t) = names();
        let foreign = self
            .foreign_keys(CatalogRequest::ForeignKeys {
                schema: s,
                table: t,
            })
            .await?;
        let referencing = self.referencing_keys(schema, table).await?;
        let mut details = meta::table_details(columns, primary, foreign, Vec::new());
        details.referenced_by = referencing;
        Ok(details)
    }

    async fn referencing_keys(&self, schema: &str, table: &str) -> Result<Vec<ReferencingKey>> {
        let rows = self
            .foreign_keys(CatalogRequest::Referencing {
                schema: schema.to_owned(),
                table: table.to_owned(),
            })
            .await?;
        Ok(meta::referencing_keys(rows))
    }

    /// The columns of each user schema, read one schema at a time: Oracle's own schemas
    /// hold far more columns than the user's.
    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>> {
        let mut out = Vec::new();
        for schema in self.schemas().await?.into_iter().filter(|s| !s.system) {
            let rows = self.columns(&schema.name, None).await?;
            out.extend(meta::catalog_columns(
                rows,
                self.kind,
                CATALOG_LIMIT - out.len(),
            ));
            if out.len() >= CATALOG_LIMIT {
                break;
            }
        }
        Ok(out)
    }

    fn transaction(&self) -> TransactionState {
        self.tx.state()
    }

    async fn set_autocommit(&self, on: bool) -> Result<()> {
        self.tx.ensure_closed()?;
        self.transaction_command(|reply| Command::Autocommit { on, reply })
            .await?;
        self.tx.set_autocommit(on);
        Ok(())
    }

    async fn commit(&self) -> Result<()> {
        self.transaction_command(|reply| Command::End {
            commit: true,
            reply,
        })
        .await?;
        self.tx.set_open(false);
        Ok(())
    }

    async fn rollback(&self) -> Result<()> {
        self.transaction_command(|reply| Command::End {
            commit: false,
            reply,
        })
        .await?;
        self.tx.set_open(false);
        Ok(())
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: true,
            cancel: true,
            ..Capabilities::default()
        }
    }
}

/// Whether a statement is waiting to start, and whether the session was given up.
#[derive(Debug, Default)]
struct CancelState {
    starting: AtomicUsize,
    abandoned: AtomicBool,
    abandon: Notify,
}

/// ODBC's `SQLCancel` is not reachable without unsafe code: a statement still waiting to
/// start is given up instead, with its session, which the app replaces; one returning rows
/// stops when its stream is dropped.
#[derive(Debug, Clone, Default)]
struct OdbcCanceller {
    state: Arc<CancelState>,
}

#[async_trait]
impl Canceller for OdbcCanceller {
    async fn cancel(&self) -> Result<()> {
        if self.state.starting.load(Ordering::SeqCst) > 0 {
            self.state.abandoned.store(true, Ordering::SeqCst);
            self.state.abandon.notify_waiters();
        }
        Ok(())
    }
}
