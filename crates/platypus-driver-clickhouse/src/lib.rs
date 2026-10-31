//! ClickHouse driver over the HTTP interface, with results in the Arrow IPC stream format.
//!
//! A connection is an HTTP session: statements carry its `session_id`, so `SET`, `USE`
//! and temporary tables last between them. One statement runs at a time, holding the
//! connection's lock until its rows are read or dropped. Each statement has a `query_id`,
//! which a separate request stops with `KILL QUERY`. ClickHouse has no transactions:
//! every statement takes effect on its own.

mod error;
mod http;
mod meta;
mod monitor;
mod security;
mod stream;
mod types;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use platypus_core::{
    CancelHandle, Canceller, Capabilities, CatalogColumn, Connection, ConnectionConfig, Driver,
    DriverKind, Error, ExecOptions, ExecResult, MetadataMatch, ObjectMeta, ObjectRef, Property,
    ReferencingKey, Result, SchemaMeta, ServerInfo, TableDetails, TransactionState, unsupported,
};
use platypus_sql::Dialect;
use tokio::sync::Mutex;

use crate::http::{Http, result_format, runs_read_only, session_params, written_rows};
use crate::stream::Running;

#[derive(Debug, Default, Clone, Copy)]
pub struct ClickHouseDriver;

#[async_trait]
impl Driver for ClickHouseDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::ClickHouse
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let conn: Box<dyn Connection> = Box::new(ClickHouseConnection::connect(config).await?);
        platypus_core::run_init(&*conn, &config.init, self.kind()).await?;
        Ok(conn)
    }
}

#[derive(Debug)]
pub struct ClickHouseConnection {
    http: Http,
    session_id: String,
    /// Held by a statement until its result is read or dropped.
    lock: Arc<Mutex<()>>,
    /// The `query_id` of the running statement, for cancelling it.
    current: Arc<StdMutex<Option<String>>>,
    server: ServerInfo,
    /// Set when the session expired or the server became unreachable.
    closed: Arc<AtomicBool>,
    /// Statements run with `readonly = 1` ([`ConnectionConfig::read_only`]).
    read_only: bool,
}

impl ClickHouseConnection {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self> {
        if config.driver != DriverKind::ClickHouse {
            return Err(Error::Config(format!(
                "{:?} is not a ClickHouse config",
                config.driver
            )));
        }
        let http = Http::new(config)?;
        let session_id = uuid::Uuid::new_v4().to_string();
        // The first request opens the session; later ones fail if it expired.
        let query_id = uuid::Uuid::new_v4().to_string();
        let response = http
            .send(
                "SELECT version() FORMAT TabSeparatedRaw",
                &session_params(&session_id, &query_id, false),
            )
            .await
            .map_err(connect_error)?;
        let version = response.text().await.map_err(error::transport)?;
        let conn = Self {
            server: ServerInfo {
                product: "ClickHouse".to_owned(),
                version: version.trim().to_owned(),
                encrypted: http.encrypted(),
            },
            http,
            session_id,
            lock: Arc::new(Mutex::new(())),
            current: Arc::new(StdMutex::new(None)),
            closed: Arc::new(AtomicBool::new(false)),
            read_only: config.read_only,
        };
        if !config.database.is_empty() {
            let sql = format!("USE {}", Dialect::ClickHouse.quote_ident(&config.database));
            conn.execute(&sql, &ExecOptions::default()).await?;
        }
        Ok(conn)
    }

    /// Records errors that mean the session is gone, so the app can reconnect.
    fn lost(&self, error: Error) -> Error {
        if matches!(error, Error::Connect(_) | Error::Closed) {
            self.closed.store(true, Ordering::Release);
        }
        error
    }

    async fn run(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let guard = Arc::clone(&self.lock).lock_owned().await;
        let query_id = uuid::Uuid::new_v4().to_string();
        if let Ok(mut current) = self.current.lock() {
            *current = Some(query_id.clone());
        }
        let running = Running {
            _guard: guard,
            current: Arc::clone(&self.current),
            http: self.http.clone(),
            query_id: query_id.clone(),
            complete: false,
        };
        let mut params = session_params(&self.session_id, &query_id, true);
        if self.read_only && runs_read_only(sql) {
            params.push(("readonly", "1"));
        }
        let columns = self.describe(sql, &params).await;
        let rewritten = types::arrow_compatible(sql, &columns);
        let sql = rewritten.as_deref().unwrap_or(sql);
        let response = match self.http.send(sql, &params).await {
            Ok(response) => response,
            Err(e) => {
                running.finish();
                return Err(e);
            }
        };
        match result_format(response.headers()).map(str::to_owned) {
            None => {
                let rows = written_rows(response.headers());
                // The body is empty; reading it ends the request.
                let read = response.bytes().await.map_err(error::transport);
                running.finish();
                read?;
                Ok(ExecResult::Affected { rows })
            }
            Some(format) if format == "ArrowStream" => Ok(ExecResult::Rows(
                stream::arrow_rows(response, options.batch_size, sql, running, &columns).await?,
            )),
            Some(format) => Ok(ExecResult::Rows(
                stream::text_rows(response, &format, running).await?,
            )),
        }
    }

    /// The declared column types of a query; empty for other statements, or when the
    /// query cannot be described (its own run then reports why).
    async fn describe(&self, sql: &str, params: &[(&str, &str)]) -> Vec<(String, String)> {
        if !types::describable(sql) {
            return Vec::new();
        }
        let Ok(response) = self.http.send(&types::describe_sql(sql), params).await else {
            return Vec::new();
        };
        match response.text().await {
            Ok(text) => types::parse_describe(&text),
            Err(_) => Vec::new(),
        }
    }
}

fn connect_error(error: Error) -> Error {
    match error {
        Error::Other(message) => Error::Connect(message),
        other => other,
    }
}

#[async_trait]
impl Connection for ClickHouseConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        // The HTTP interface rejects a trailing `;`.
        let sql = sql.trim_end().trim_end_matches(';');
        self.run(sql, options).await.map_err(|e| self.lost(e))
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(ClickHouseCanceller {
            http: self.http.clone(),
            current: Arc::clone(&self.current),
        })
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

    /// ClickHouse has no foreign keys.
    async fn referencing_keys(&self, _schema: &str, _table: &str) -> Result<Vec<ReferencingKey>> {
        Ok(Vec::new())
    }

    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>> {
        meta::catalog_columns(self).await
    }

    fn transaction(&self) -> TransactionState {
        TransactionState {
            autocommit: true,
            open: false,
        }
    }

    async fn set_autocommit(&self, on: bool) -> Result<()> {
        if on {
            Ok(())
        } else {
            Err(unsupported(
                "manual commit: ClickHouse has no transactions, each statement takes effect on its own",
            ))
        }
    }

    async fn commit(&self) -> Result<()> {
        Ok(())
    }

    async fn rollback(&self) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: false,
            cancel: true,
            explain: true,
            object_ddl: true,
            metadata_search: true,
            server_parameters: true,
            server_metrics: true,
            security: true,
            ..Capabilities::default()
        }
    }

    async fn server_parameters(&self) -> Result<Vec<platypus_core::ServerParameter>> {
        monitor::parameters(self).await
    }

    async fn server_metrics(&self) -> Result<platypus_core::ServerMetrics> {
        monitor::metrics(self).await
    }

    async fn principals(&self) -> Result<Vec<platypus_core::Principal>> {
        security::principals(self).await
    }

    async fn principal_privileges(
        &self,
        name: &str,
        _host: Option<&str>,
    ) -> Result<Vec<platypus_core::PrivilegeGrant>> {
        security::privileges(self, name).await
    }

    async fn object_ddl(&self, object: &ObjectRef<'_>) -> Result<String> {
        meta::object_ddl(self, object).await
    }

    /// `EXPLAIN indexes = 1`, a tree indented by two spaces. ClickHouse cannot run a
    /// statement for its actual rows and times: with `analyze` the plan comes without them.
    async fn explain(&self, sql: &str, _analyze: bool) -> Result<platypus_core::Explain> {
        let plan =
            platypus_core::explain_document(self, &format!("EXPLAIN indexes = 1 {sql}"), false)
                .await?;
        let root = platypus_core::plan_from_text(&plan, |line| {
            let text = line.trim_start();
            (!text.is_empty()).then(|| (line.len() - text.len(), text.to_owned()))
        });
        Ok(platypus_core::Explain::new(
            platypus_core::ExplainFormat::Text,
            plan,
            root,
            false,
        ))
    }

    async fn object_properties(&self, object: &ObjectRef<'_>) -> Result<Vec<Property>> {
        meta::object_properties(self, object).await
    }

    async fn search_metadata(&self, needle: &str) -> Result<Vec<MetadataMatch>> {
        meta::search_metadata(self, needle).await
    }
}

#[derive(Debug, Clone)]
struct ClickHouseCanceller {
    http: Http,
    current: Arc<StdMutex<Option<String>>>,
}

#[async_trait]
impl Canceller for ClickHouseCanceller {
    async fn cancel(&self) -> Result<()> {
        let running = self.current.lock().ok().and_then(|c| c.clone());
        match running {
            Some(query_id) => self.http.kill(&query_id).await,
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
