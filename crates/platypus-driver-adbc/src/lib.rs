//! Databases reached through an ADBC driver loaded at run time: Snowflake, `BigQuery`,
//! Databricks, Trino, and any ADBC driver the user points at. The drivers of the named
//! databases are downloaded by the app (`platypus-drivers`); this crate loads them.

use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};

use adbc_core::Connection as _;
use adbc_core::options::ObjectDepth;
use adbc_driver_manager::ManagedConnection;
use arrow_array::RecordBatch;
use async_trait::async_trait;
use futures::StreamExt as _;
use platypus_core::{
    CATALOG_LIMIT, CancelHandle, Canceller, Capabilities, CatalogColumn, Connection,
    ConnectionConfig, Driver, DriverKind, Error, ExecOptions, ExecResult, ObjectMeta,
    RecordBatchStream, ReferencingKey, Result, SchemaMeta, ServerInfo, TableDetails,
    TransactionState,
};
use tokio::sync::oneshot;

mod ipc;
mod meta;
pub mod profile;
mod session;

use session::{Command, Started};

/// Opens connections through ADBC drivers; the downloaded ones live under `data_dir`.
#[derive(Debug, Clone)]
pub struct AdbcDriver {
    pub data_dir: PathBuf,
}

impl AdbcDriver {
    /// With the drivers of the app's data folder (`platypus_drivers::set_data_dir`).
    pub fn shared() -> Result<Self> {
        platypus_drivers::data_dir()
            .map(|dir| Self {
                data_dir: dir.to_owned(),
            })
            .ok_or_else(|| Error::Config("the drivers folder is not set".to_owned()))
    }
}

/// The library that opens `config`: the downloaded driver of its database, or the file the
/// user chose for the generic driver.
pub fn library(config: &ConnectionConfig, data_dir: &Path) -> Result<PathBuf> {
    if let Some(name) = config.driver.adbc_package() {
        return platypus_drivers::installed(data_dir, name)
            .map(|i| i.library)
            .ok_or(Error::DriverMissing {
                driver: config.driver,
            });
    }
    let file = config
        .options
        .get(profile::DRIVER_FILE)
        .map(|f| f.trim())
        .filter(|f| !f.is_empty())
        .ok_or_else(|| Error::Config("choose the ADBC driver's library file".to_owned()))?;
    let path = PathBuf::from(file);
    if path.is_file() {
        Ok(path)
    } else {
        Err(Error::Config(format!("no driver library at {file}")))
    }
}

#[async_trait]
impl Driver for AdbcDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Adbc
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let library = library(config, &self.data_dir)?;
        let conn: Box<dyn Connection> = Box::new(AdbcConnection::open(config, &library).await?);
        platypus_core::run_init(&*conn, &config.init, config.driver).await?;
        Ok(conn)
    }
}

#[derive(Debug)]
pub struct AdbcConnection {
    commands: Mutex<std_mpsc::Sender<Command>>,
    canceller: AdbcCanceller,
    server: ServerInfo,
    /// The catalog the metadata is read from: the connection's database, or where the
    /// session started.
    catalog: Option<String>,
    schema: Option<String>,
}

impl AdbcConnection {
    pub async fn open(config: &ConnectionConfig, library: &Path) -> Result<Self> {
        let options = profile::options(config)?;
        let (commands, opened) = session::spawn(library, options)?;
        let opened = opened.await.map_err(|_| Error::Closed)??;
        let configured = |key: &str| {
            config
                .options
                .get(key)
                .filter(|v| !v.trim().is_empty())
                .cloned()
        };
        let catalog = match config.driver {
            DriverKind::BigQuery => configured("project"),
            DriverKind::Snowflake | DriverKind::Trino | DriverKind::Databricks => {
                Some(config.database.clone()).filter(|d| !d.is_empty())
            }
            _ => None,
        }
        .or(opened.catalog);
        let schema = configured(if config.driver == DriverKind::BigQuery {
            "dataset"
        } else {
            "schema"
        })
        .or(opened.schema);
        Ok(Self {
            commands: Mutex::new(commands),
            canceller: AdbcCanceller(Arc::new(opened.cancel)),
            server: ServerInfo {
                product: match config.driver {
                    DriverKind::Adbc => opened.vendor.unwrap_or_else(|| "ADBC".to_owned()),
                    named => named.name().to_owned(),
                },
                version: opened.version.unwrap_or_default(),
                // The drivers choose; Trino and Databricks speak HTTPS unless told not to.
                encrypted: config.ssl_mode != platypus_core::SslMode::Disable,
            },
            catalog,
            schema,
        })
    }

    fn send(&self, command: Command) -> Result<()> {
        self.commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .send(command)
            .map_err(|_| Error::Closed)
    }

    async fn objects_of(
        &self,
        depth: ObjectDepth,
        schema: Option<&str>,
        table: Option<&str>,
    ) -> Result<Vec<meta::Catalog>> {
        let (reply, answer) = oneshot::channel();
        self.send(Command::Objects {
            depth,
            catalog: self.catalog.clone(),
            schema: schema.map(str::to_owned),
            table: table.map(str::to_owned),
            reply,
        })?;
        let batches: Vec<RecordBatch> = answer.await.map_err(|_| Error::Closed)??;
        meta::parse_objects(&batches)
    }
}

#[async_trait]
impl Connection for AdbcConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let (reply, started) = oneshot::channel();
        self.send(Command::Execute {
            sql: sql.to_owned(),
            batch_size: options.batch_size.max(1),
            reply,
        })?;
        match started.await.map_err(|_| Error::Closed)?? {
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

    fn is_closed(&self) -> bool {
        false
    }

    async fn schemas(&self) -> Result<Vec<SchemaMeta>> {
        let catalogs = self.objects_of(ObjectDepth::Schemas, None, None).await?;
        Ok(meta::schemas(&catalogs, self.schema.as_deref()))
    }

    async fn objects(&self, schema: &str) -> Result<Vec<ObjectMeta>> {
        let catalogs = self
            .objects_of(ObjectDepth::Tables, Some(schema), None)
            .await?;
        Ok(meta::objects(&catalogs))
    }

    async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails> {
        let catalogs = self
            .objects_of(ObjectDepth::All, Some(schema), Some(table))
            .await?;
        Ok(meta::table_details(&catalogs))
    }

    /// Finding them means reading every table's keys: not offered.
    async fn referencing_keys(&self, _schema: &str, _table: &str) -> Result<Vec<ReferencingKey>> {
        Ok(Vec::new())
    }

    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>> {
        let catalogs = self.objects_of(ObjectDepth::All, None, None).await?;
        Ok(meta::catalog_columns(&catalogs, CATALOG_LIMIT))
    }

    /// Every statement commits on its own: transactions differ too much between drivers.
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
            Err(Error::Unsupported(
                "manual commit with an ADBC driver".to_owned(),
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
            ..Capabilities::default()
        }
    }
}

/// Cancels through a handle on the same connection: the worker holds only the statement's
/// lock while it runs.
#[derive(Clone)]
struct AdbcCanceller(Arc<ManagedConnection>);

impl std::fmt::Debug for AdbcCanceller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AdbcCanceller")
    }
}

#[async_trait]
impl Canceller for AdbcCanceller {
    async fn cancel(&self) -> Result<()> {
        let connection = Arc::clone(&self.0);
        tokio::task::spawn_blocking(move || {
            let mut connection = (*connection).clone();
            // Drivers without cancellation report it; nothing else to do then.
            let _ = connection.cancel();
        })
        .await
        .map_err(|e| Error::Unsupported(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_driver_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = ConnectionConfig::new(DriverKind::Snowflake);
        assert!(matches!(
            library(&config, dir.path()),
            Err(Error::DriverMissing {
                driver: DriverKind::Snowflake
            })
        ));
        config.driver = DriverKind::Adbc;
        assert!(matches!(
            library(&config, dir.path()),
            Err(Error::Config(_))
        ));
        let file = dir.path().join("x.dll");
        std::fs::write(&file, b"").unwrap();
        config.options.insert(
            profile::DRIVER_FILE.to_owned(),
            file.to_string_lossy().into_owned(),
        );
        assert_eq!(library(&config, dir.path()).unwrap(), file);
    }
}
