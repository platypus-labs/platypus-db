//! Elasticsearch and `OpenSearch` driver over the HTTP API.
//!
//! The editor takes requests in the form of Kibana's Dev Tools console, and SQL
//! ([`command`]). SQL goes to the SQL endpoint of the product (`_sql` on Elasticsearch,
//! `_plugins/_sql` on `OpenSearch`) and comes back as typed columns ([`sqlrows`]); a search
//! gives documents, laid out as the MongoDB driver lays them out ([`doc`], [`reply`]), read
//! page by page when it has many ([`paging`]). A cluster is one schema, its indices the
//! tables ([`meta`]); [`edit`] turns the grid's changes into requests.
//!
//! Dropping a request stops a search on the server, which is how the cancel handle stops a
//! statement. There are no transactions.

pub mod command;
mod doc;
pub mod edit;
mod error;
mod http;
mod kind;
pub mod meta;
mod paging;
mod reply;
mod session;
mod sqlrows;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use platypus_core::{
    CancelHandle, Canceller, Capabilities, CatalogColumn, Connection, ConnectionConfig, Driver,
    DriverKind, Error, ExecOptions, ExecResult, ObjectMeta, ObjectRef, Property, ReferencingKey,
    Result, SchemaMeta, ServerInfo, ServerVersion, TableDetails, TransactionState, unsupported,
};
use serde_json::Value;
use tokio::sync::watch;

use crate::command::Command;
use crate::http::{Http, Payload};
use crate::session::{Flavor, Shared};

/// Options of a connection that belong to the driver: `database` is an index pattern
/// (`logs-*`) that narrows what the navigator lists.
#[derive(Debug, Default, Clone, Copy)]
pub struct SearchDriver;

#[async_trait]
impl Driver for SearchDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Elasticsearch
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let conn: Box<dyn Connection> = Box::new(SearchConnection::connect(config).await?);
        platypus_core::run_init(&*conn, &config.init, self.kind()).await?;
        Ok(conn)
    }
}

#[derive(Debug)]
pub struct SearchConnection {
    shared: Arc<Shared>,
    info: ServerInfo,
    cluster: String,
    /// The index pattern the navigator lists; empty for all.
    pattern: String,
}

/// What the server says of itself at `GET /`.
fn identify(root: &Value) -> Option<(Flavor, String, String)> {
    let version = root.get("version")?;
    let number = version.get("number")?.as_str()?.to_owned();
    let flavor = if version.get("distribution").and_then(Value::as_str) == Some("opensearch") {
        Flavor::OpenSearch
    } else {
        Flavor::Elasticsearch
    };
    let cluster = root
        .get("cluster_name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("default")
        .to_owned();
    Some((flavor, number, cluster))
}

/// A failed first request is a failed connection, whatever the server said.
fn connect_error(error: Error) -> Error {
    match error {
        Error::Database(db) => Error::Connect(match db.code {
            Some(code) => format!("{}: {}", code, db.message),
            None => db.message,
        }),
        Error::Other(message) => Error::Connect(message),
        other => other,
    }
}

impl SearchConnection {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self> {
        if config.driver != DriverKind::Elasticsearch {
            return Err(Error::Config(format!(
                "{:?} is not an Elasticsearch config",
                config.driver
            )));
        }
        let http = Http::new(config)?;
        let reply = http
            .call("GET", "/", Payload::None, None)
            .await
            .map_err(connect_error)?;
        let root: Value = serde_json::from_str(&reply.body).map_err(|_| {
            Error::Connect("the server is not Elasticsearch or OpenSearch".to_owned())
        })?;
        let (flavor, number, cluster) = identify(&root).ok_or_else(|| {
            Error::Connect("the server is not Elasticsearch or OpenSearch".to_owned())
        })?;
        let info = ServerInfo {
            product: flavor.product().to_owned(),
            version: number,
            encrypted: http.encrypted(),
        };
        if let Some(minimum) = info.below_minimum() {
            return Err(Error::Connect(format!(
                "{} {} is older than {minimum}, the oldest version supported",
                info.product, info.version
            )));
        }
        let version = info
            .version_number()
            .unwrap_or_else(|| ServerVersion::new(0, 0, 0));
        Ok(Self {
            shared: Arc::new(Shared {
                http,
                flavor,
                version,
                cancel: watch::channel(0).0,
                closed: AtomicBool::new(false),
            }),
            info,
            cluster,
            pattern: config.database.trim().to_owned(),
        })
    }

    async fn run(&self, command: Command, options: &ExecOptions) -> Result<ExecResult> {
        match command {
            Command::Sql(sql) => sqlrows::run(&self.shared, &sql, options).await,
            Command::Request(request) => {
                if let Some(plan) = paging::plan(&request, options.batch_size) {
                    return paging::run(&self.shared, plan).await;
                }
                reply::run(&self.shared, &request, options.batch_size).await
            }
        }
    }
}

#[async_trait]
impl Connection for SearchConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let ranges = command::split(sql);
        if ranges.is_empty() {
            return Err(error::parse_error(sql, "no statement"));
        }
        let mut last = None;
        for range in ranges {
            let text = &sql[range];
            let command = command::parse(text).map_err(|e| error::parse_error(text, &e))?;
            last = Some(self.run(command, options).await?);
        }
        last.ok_or_else(|| Error::Other("no statement".to_owned()))
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(SearchCanceller {
            shared: Arc::clone(&self.shared),
        })
    }

    fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Relaxed)
    }

    async fn check_alive(&self) -> bool {
        self.shared
            .call("GET", "/", Payload::None, None)
            .await
            .is_ok()
    }

    async fn schemas(&self) -> Result<Vec<SchemaMeta>> {
        Ok(meta::schemas(&self.cluster))
    }

    async fn objects(&self, _schema: &str) -> Result<Vec<ObjectMeta>> {
        meta::objects(&self.shared, &self.pattern).await
    }

    async fn table_details(&self, _schema: &str, table: &str) -> Result<TableDetails> {
        meta::table_details(&self.shared, table).await
    }

    /// Indices have no foreign keys.
    async fn referencing_keys(&self, _schema: &str, _table: &str) -> Result<Vec<ReferencingKey>> {
        Ok(Vec::new())
    }

    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>> {
        meta::catalog_columns(&self.shared, &self.cluster, &self.pattern).await
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
                "manual commit: Elasticsearch has no transactions, each request takes effect on its own",
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
            cancel: true,
            object_ddl: true,
            ..Capabilities::default()
        }
    }

    async fn object_ddl(&self, object: &ObjectRef<'_>) -> Result<String> {
        meta::object_ddl(&self.shared, object).await
    }

    async fn object_properties(&self, object: &ObjectRef<'_>) -> Result<Vec<Property>> {
        meta::object_properties(&self.shared, object).await
    }
}

struct SearchCanceller {
    shared: Arc<Shared>,
}

#[async_trait]
impl Canceller for SearchCanceller {
    async fn cancel(&self) -> Result<()> {
        self.shared.cancel.send_modify(|n| *n += 1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_the_products_apart() {
        let es: Value = serde_json::from_str(
            r#"{"name":"n","cluster_name":"docker-cluster","version":{"number":"8.15.3","build_flavor":"default"},"tagline":"You Know, for Search"}"#,
        )
        .unwrap();
        let (flavor, version, cluster) = identify(&es).unwrap();
        assert_eq!(
            (flavor, version.as_str(), cluster.as_str()),
            (Flavor::Elasticsearch, "8.15.3", "docker-cluster")
        );
        let os: Value = serde_json::from_str(
            r#"{"cluster_name":"","version":{"distribution":"opensearch","number":"2.17.1"}}"#,
        )
        .unwrap();
        let (flavor, _, cluster) = identify(&os).unwrap();
        assert_eq!((flavor, cluster.as_str()), (Flavor::OpenSearch, "default"));
        assert!(identify(&serde_json::json!({"status": "ok"})).is_none());
    }
}
