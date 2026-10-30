//! Apache Cassandra and `ScyllaDB` driver over CQL, on the `scylla` crate (which speaks to both).
//!
//! The editor takes CQL, a script of statements ([`cql`]); a result comes back as typed Arrow
//! columns, page by page with the server's paging state ([`exec`], [`columns`]). Keyspaces are
//! the schemas, their tables the tables ([`meta`]); [`edit`] turns the grid's changes into
//! statements by the full primary key.
//!
//! The connection uses the node it is given as the coordinator of every statement: a cluster
//! advertises its nodes' own addresses, which a container, a NAT or a tunnel hides. There are
//! no transactions: each statement takes effect on its own. A write reports no row count (CQL
//! writes are upserts), so a statement that returns no rows reports none changed.

mod columns;
pub mod cql;
pub mod edit;
mod error;
mod exec;
pub mod meta;
pub mod render;
mod session;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use platypus_core::{
    CancelHandle, Canceller, Capabilities, CatalogColumn, Connection, ConnectionConfig, Driver,
    DriverKind, Error, ExecOptions, ExecResult, ObjectMeta, ObjectRef, Property, ReferencingKey,
    Result, SchemaMeta, ServerInfo, TableDetails, TransactionState, unsupported,
};

use crate::session::Shared;

/// The driver for Cassandra and `ScyllaDB`.
#[derive(Debug, Default, Clone, Copy)]
pub struct CassandraDriver;

#[async_trait]
impl Driver for CassandraDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Cassandra
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let conn: Box<dyn Connection> = Box::new(CassandraConnection::connect(config).await?);
        platypus_core::run_init(&*conn, &config.init, self.kind()).await?;
        Ok(conn)
    }
}

#[derive(Debug)]
pub struct CassandraConnection {
    shared: Arc<Shared>,
    info: ServerInfo,
}

impl CassandraConnection {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self> {
        if config.driver != DriverKind::Cassandra {
            return Err(Error::Config(format!(
                "{:?} is not a Cassandra config",
                config.driver
            )));
        }
        let (shared, info) = session::open(config).await?;
        Ok(Self {
            shared: Arc::new(shared),
            info,
        })
    }
}

#[async_trait]
impl Connection for CassandraConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let ranges = cql::split(sql);
        let Some((last, earlier)) = ranges.split_last() else {
            return Err(error::parse_error("no statement"));
        };
        for range in earlier {
            exec::run(&self.shared, &sql[range.clone()], &ExecOptions::default()).await?;
        }
        exec::run(&self.shared, &sql[last.clone()], options).await
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(CassandraCanceller {
            shared: Arc::clone(&self.shared),
        })
    }

    fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Relaxed)
    }

    fn take_messages(&self) -> Vec<String> {
        self.shared
            .messages
            .lock()
            .map(|mut m| std::mem::take(&mut *m))
            .unwrap_or_default()
    }

    async fn check_alive(&self) -> bool {
        self.execute("SELECT key FROM system.local", &ExecOptions::default())
            .await
            .is_ok()
    }

    async fn schemas(&self) -> Result<Vec<SchemaMeta>> {
        meta::schemas(self, self.shared.current_keyspace().as_deref()).await
    }

    async fn objects(&self, schema: &str) -> Result<Vec<ObjectMeta>> {
        meta::objects(self, schema).await
    }

    async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails> {
        meta::table_details(self, schema, table).await
    }

    /// Tables have no foreign keys.
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
                "manual commit: Cassandra has no transactions, each statement takes effect on its own",
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
        meta::object_ddl(self, object).await
    }

    async fn object_properties(&self, object: &ObjectRef<'_>) -> Result<Vec<Property>> {
        meta::object_properties(self, object).await
    }
}

struct CassandraCanceller {
    shared: Arc<Shared>,
}

#[async_trait]
impl Canceller for CassandraCanceller {
    async fn cancel(&self) -> Result<()> {
        self.shared.cancel.send_modify(|n| *n += 1);
        Ok(())
    }
}
