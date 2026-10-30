//! Plugin drivers: a connection whose statements a plugin runs (`DriverKind::Plugin`, the
//! plugin's id in the connection's `plugin` option).
//!
//! Each connection has its own sandbox, so the guest keeps its session in its memory from
//! one call to the next. The guest's `platypus_driver` receives one operation at a time:
//!
//! - `{"op": "connect", "config": {...}, "secrets": {"password": ...}}`: the connection's
//!   fields and options; the password only here, never stored by the host in the guest.
//!   Answers `{"product": ..., "version": ...}`.
//! - `{"op": "execute", "sql": ...}`: answers `{"columns": [{"name", "type"}], "rows":
//!   [[...]]}` for rows, or `{"affected": n}`. The host makes Arrow batches of the rows.
//! - `{"op": "schemas"}`, `{"op": "objects", "schema"}`, `{"op": "table", "schema",
//!   "table"}`: the navigator, when the driver says `"metadata": true`.
//! - `{"op": "close"}`, when the connection is dropped.
//!
//! The guest reaches its database only with the `http` host function: the hosts of its
//! permissions, and the connection's own host, which the user typed. Raw TCP is not
//! possible, so databases with an HTTP API only. No transactions and no cancel.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use arrow_array::builder::{BooleanBuilder, Float64Builder, Int64Builder, StringBuilder};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use async_trait::async_trait;
use platypus_core::{
    CancelHandle, Canceller, Capabilities, CatalogColumn, ColumnMeta, Connection, ConnectionConfig,
    Driver, DriverKind, Error, ExecOptions, ExecResult, ObjectKind, ObjectMeta, RecordBatchStream,
    ReferencingKey, Result, SchemaMeta, ServerInfo, TableDetails, TransactionState, unsupported,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::PluginError;
use crate::manifest::Kind;
use crate::package::{Plugins, blocking};
use crate::runtime::{self, DRIVER, Grants, Sandbox};

/// The connection option with the id of the plugin that is its driver.
pub const PLUGIN_OPTION: &str = "plugin";

/// Options that stay with the host: they may carry credentials (a proxy's) or commands.
const HOST_OPTIONS: [&str; 4] = [
    PLUGIN_OPTION,
    platypus_core::BEFORE_CONNECT,
    platypus_core::AFTER_DISCONNECT,
    "proxy",
];

/// A plugin's driver, as the plugin describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverInfo {
    /// The database's name, shown in the driver picker.
    pub name: String,
    /// It answers `schemas`, `objects` and `table`, for the navigator.
    #[serde(default)]
    pub metadata: bool,
}

pub(crate) fn check(info: &DriverInfo) -> Result<(), PluginError> {
    if info.name.trim().is_empty() {
        return Err(PluginError::Invalid("the driver has no name".to_owned()));
    }
    Ok(())
}

/// Opens connections through the installed plugin named in their options.
#[derive(Debug, Clone)]
pub struct PluginDriver {
    plugins: Plugins,
}

impl PluginDriver {
    #[must_use]
    pub fn new(plugins: Plugins) -> Self {
        Self { plugins }
    }

    /// With the plugins of the app's data folder.
    pub fn shared() -> Result<Self> {
        Ok(Self::new(Plugins::shared()?))
    }
}

#[async_trait]
impl Driver for PluginDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Plugin
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let conn: Box<dyn Connection> =
            Box::new(PluginConnection::connect(&self.plugins, config).await?);
        platypus_core::run_init(&*conn, &config.init, self.kind()).await?;
        Ok(conn)
    }
}

/// What `connect` answers.
#[derive(Debug, Default, Deserialize)]
struct Hello {
    #[serde(default)]
    product: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

/// A connection run by a plugin.
pub struct PluginConnection {
    sandbox: Arc<Mutex<Sandbox>>,
    server: ServerInfo,
    metadata: bool,
    closed: Arc<AtomicBool>,
}

impl std::fmt::Debug for PluginConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginConnection")
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}

impl PluginConnection {
    pub async fn connect(plugins: &Plugins, config: &ConnectionConfig) -> Result<Self> {
        let id = config
            .options
            .get(PLUGIN_OPTION)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| Error::Config("no plugin is chosen as the driver".to_owned()))?
            .clone();
        let (installed, wasm) = plugins.load(&id)?;
        let Some(info) = installed
            .description
            .driver
            .clone()
            .filter(|_| installed.manifest.provides(Kind::Driver))
        else {
            return Err(Error::Config(format!("the plugin {id:?} has no driver")));
        };
        let mut http = installed.approved.http.clone();
        if !config.host.trim().is_empty() {
            http.push(config.host.trim().to_owned());
        }
        let grants = Grants {
            plugin: id,
            http,
            proxy: config.options.get("proxy").cloned(),
        };
        let request = connect_request(config);
        let limits = plugins.limits();
        let (sandbox, hello) = blocking(move || {
            let module = runtime::compile(&wasm)?;
            let mut sandbox = Sandbox::new(&module, &grants, limits)?;
            let hello: Hello = sandbox.call(DRIVER, &request)?;
            Ok((sandbox, hello))
        })
        .await
        .map_err(|e| match e {
            PluginError::Guest(message) => Error::Connect(message),
            other => other.into(),
        })?;
        Ok(Self {
            sandbox: Arc::new(Mutex::new(sandbox)),
            server: ServerInfo {
                product: hello.product.unwrap_or(info.name),
                version: hello.version.unwrap_or_default(),
                // The plugin's requests may or may not use TLS: the host cannot tell.
                encrypted: false,
            },
            metadata: info.metadata,
            closed: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Runs one operation on the plugin's sandbox.
    async fn op<T: serde::de::DeserializeOwned + Send + 'static>(
        &self,
        request: Value,
    ) -> Result<T> {
        if self.is_closed() {
            return Err(Error::Closed);
        }
        let sandbox = Arc::clone(&self.sandbox);
        let closed = Arc::clone(&self.closed);
        blocking(move || {
            let mut sandbox = sandbox
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let result = sandbox.call(DRIVER, &request);
            if sandbox.is_broken() {
                closed.store(true, Ordering::Release);
            }
            result
        })
        .await
        .map_err(Error::from)
    }
}

/// The `connect` operation: the configuration without its secrets, and the secrets apart.
fn connect_request(config: &ConnectionConfig) -> Value {
    let options: BTreeMap<&String, &String> = config
        .options
        .iter()
        .filter(|(k, _)| !HOST_OPTIONS.contains(&k.as_str()))
        .collect();
    json!({
        "op": "connect",
        "config": {
            "host": config.host,
            "port": config.port,
            "database": config.database,
            "user": config.user,
            "ssl_mode": config.ssl_mode.as_str(),
            "options": options,
        },
        "secrets": {
            "password": config.password.as_ref().map(platypus_core::Secret::expose),
        },
    })
}

impl Drop for PluginConnection {
    fn drop(&mut self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        // The guest may log out over HTTP: on a blocking thread, without waiting for it.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let sandbox = Arc::clone(&self.sandbox);
            runtime.spawn_blocking(move || {
                let mut sandbox = sandbox
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let _ = sandbox.call::<_, Value>(DRIVER, &json!({ "op": "close" }));
            });
        }
    }
}

/// An `execute` answer.
#[derive(Debug, Deserialize)]
struct Answer {
    #[serde(default)]
    columns: Option<Vec<ColumnAnswer>>,
    #[serde(default)]
    rows: Vec<Vec<Value>>,
    #[serde(default)]
    affected: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ColumnAnswer {
    name: String,
    #[serde(default, rename = "type")]
    ty: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SchemaAnswer {
    name: String,
    #[serde(default)]
    current: bool,
}

#[derive(Debug, Deserialize)]
struct ObjectAnswer {
    name: String,
    #[serde(default = "table_kind")]
    kind: ObjectKind,
    #[serde(default)]
    comment: Option<String>,
}

const fn table_kind() -> ObjectKind {
    ObjectKind::Table
}

#[derive(Debug, Deserialize)]
struct TableAnswer {
    #[serde(default)]
    columns: Vec<TableColumnAnswer>,
}

#[derive(Debug, Deserialize)]
struct TableColumnAnswer {
    name: String,
    #[serde(default, rename = "type")]
    ty: String,
    #[serde(default = "yes")]
    nullable: bool,
    #[serde(default)]
    primary_key: bool,
    #[serde(default)]
    comment: Option<String>,
}

const fn yes() -> bool {
    true
}

#[async_trait]
impl Connection for PluginConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let answer: Answer = self.op(json!({ "op": "execute", "sql": sql })).await?;
        let Some(columns) = answer.columns else {
            return Ok(ExecResult::Affected {
                rows: answer.affected.unwrap_or(0),
            });
        };
        let (schema, batches) = to_batches(&columns, &answer.rows, options.batch_size.max(1))?;
        Ok(ExecResult::Rows(RecordBatchStream::new(
            schema,
            Box::pin(futures::stream::iter(batches.into_iter().map(Ok))),
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
        if !self.metadata {
            return Ok(Vec::new());
        }
        let schemas: Vec<SchemaAnswer> = self.op(json!({ "op": "schemas" })).await?;
        Ok(schemas
            .into_iter()
            .map(|s| SchemaMeta {
                name: s.name,
                system: false,
                current: s.current,
            })
            .collect())
    }

    async fn objects(&self, schema: &str) -> Result<Vec<ObjectMeta>> {
        if !self.metadata {
            return Ok(Vec::new());
        }
        let objects: Vec<ObjectAnswer> = self
            .op(json!({ "op": "objects", "schema": schema }))
            .await?;
        Ok(objects
            .into_iter()
            .map(|o| ObjectMeta::new(o.name, o.kind, o.comment))
            .collect())
    }

    async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails> {
        if !self.metadata {
            return Err(unsupported("table details of this plugin driver"));
        }
        let answer: TableAnswer = self
            .op(json!({ "op": "table", "schema": schema, "table": table }))
            .await?;
        Ok(TableDetails {
            columns: answer
                .columns
                .into_iter()
                .map(|c| ColumnMeta {
                    name: c.name,
                    data_type: c.ty,
                    nullable: c.nullable,
                    default: None,
                    primary_key: c.primary_key,
                    comment: c.comment,
                    enum_values: Vec::new(),
                    fields: Vec::new(),
                })
                .collect(),
            ..TableDetails::default()
        })
    }

    async fn referencing_keys(&self, _schema: &str, _table: &str) -> Result<Vec<ReferencingKey>> {
        Ok(Vec::new())
    }

    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>> {
        Ok(Vec::new())
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
            Err(unsupported("transactions with a plugin driver"))
        }
    }

    async fn commit(&self) -> Result<()> {
        Ok(())
    }

    async fn rollback(&self) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
}

/// A plugin cannot be interrupted from outside: its fuel and deadline stop it.
struct NoCancel;

#[async_trait]
impl Canceller for NoCancel {
    async fn cancel(&self) -> Result<()> {
        Err(unsupported("cancelling a plugin driver's statements"))
    }
}

/// The Arrow type of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Type {
    Bool,
    Int,
    Float,
    Text,
}

impl Type {
    /// The type a plugin declared, when it is one the host maps.
    fn declared(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "bool" | "boolean" => Some(Self::Bool),
            "int" | "integer" | "bigint" | "long" => Some(Self::Int),
            "float" | "double" | "real" | "number" | "numeric" | "decimal" => Some(Self::Float),
            "text" | "string" | "varchar" => Some(Self::Text),
            _ => None,
        }
    }

    fn fits(self, value: &Value) -> bool {
        match (self, value) {
            (_, Value::Null)
            | (Self::Text, _)
            | (Self::Float, Value::Number(_))
            | (Self::Bool, Value::Bool(_)) => true,
            (Self::Int, Value::Number(n)) => n.as_i64().is_some(),
            _ => false,
        }
    }

    /// The narrowest type every value fits.
    fn inferred<'a>(values: &(impl Iterator<Item = &'a Value> + Clone)) -> Self {
        [Self::Bool, Self::Int, Self::Float]
            .into_iter()
            .find(|t| {
                let mut values = values.clone().filter(|v| !v.is_null()).peekable();
                values.peek().is_some() && values.all(|v| t.fits(v))
            })
            .unwrap_or(Self::Text)
    }

    fn arrow(self) -> DataType {
        match self {
            Self::Bool => DataType::Boolean,
            Self::Int => DataType::Int64,
            Self::Float => DataType::Float64,
            Self::Text => DataType::Utf8,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::Text => "text",
        }
    }
}

/// The rows of an answer as batches of at most `batch_size` rows. A column takes its
/// declared type when every value fits it, else the narrowest that all fit; text shows
/// any other JSON value as JSON.
fn to_batches(
    columns: &[ColumnAnswer],
    rows: &[Vec<Value>],
    batch_size: usize,
) -> Result<(SchemaRef, Vec<RecordBatch>)> {
    if let Some(row) = rows.iter().find(|r| r.len() != columns.len()) {
        return Err(Error::Other(format!(
            "the plugin answered a row of {} values for {} columns",
            row.len(),
            columns.len()
        )));
    }
    let types: Vec<Type> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let values = rows.iter().map(move |r| &r[i]);
            c.ty.as_deref()
                .and_then(Type::declared)
                .filter(|t| values.clone().all(|v| t.fits(v)))
                .unwrap_or_else(|| Type::inferred(&values))
        })
        .collect();
    let fields: Vec<Field> = columns
        .iter()
        .zip(&types)
        .map(|(c, t)| {
            let db_type = c.ty.clone().unwrap_or_else(|| t.name().to_owned());
            Field::new(&c.name, t.arrow(), true)
                .with_metadata([(platypus_core::META_DB_TYPE, db_type)])
        })
        .collect();
    let schema = Arc::new(Schema::new(fields));
    let batches = rows
        .chunks(batch_size)
        .map(|chunk| {
            let arrays = types
                .iter()
                .enumerate()
                .map(|(i, t)| array(*t, chunk.iter().map(|r| &r[i])))
                .collect();
            RecordBatch::try_new(Arc::clone(&schema), arrays).map_err(Error::from)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((schema, batches))
}

fn array<'a>(ty: Type, values: impl Iterator<Item = &'a Value>) -> ArrayRef {
    match ty {
        Type::Bool => {
            let mut b = BooleanBuilder::new();
            values.for_each(|v| b.append_option(v.as_bool()));
            Arc::new(b.finish())
        }
        Type::Int => {
            let mut b = Int64Builder::new();
            values.for_each(|v| b.append_option(v.as_i64()));
            Arc::new(b.finish())
        }
        Type::Float => {
            let mut b = Float64Builder::new();
            values.for_each(|v| b.append_option(v.as_f64()));
            Arc::new(b.finish())
        }
        Type::Text => {
            let mut b = StringBuilder::new();
            values.for_each(|v| match v {
                Value::Null => b.append_null(),
                Value::String(s) => b.append_value(s),
                other => b.append_value(other.to_string()),
            });
            Arc::new(b.finish())
        }
    }
}

#[cfg(test)]
mod tests {
    use arrow_array::cast::AsArray;
    use arrow_array::types::{Float64Type, Int64Type};

    use super::*;

    fn columns(spec: &[(&str, Option<&str>)]) -> Vec<ColumnAnswer> {
        spec.iter()
            .map(|(name, ty)| ColumnAnswer {
                name: (*name).to_owned(),
                ty: ty.map(str::to_owned),
            })
            .collect()
    }

    #[test]
    fn types_columns_from_declarations_and_values() {
        let cols = columns(&[
            ("id", Some("int")),
            ("price", None),
            ("ok", None),
            ("name", Some("varchar(20)")),
            ("bad", Some("int")),
            ("doc", None),
        ]);
        let rows = vec![
            vec![
                json!(1),
                json!(1),
                json!(true),
                json!("a"),
                json!("x"),
                json!({"a": 1}),
            ],
            vec![
                json!(2),
                json!(2.5),
                json!(null),
                json!(null),
                json!(3),
                json!([1]),
            ],
        ];
        let (schema, batches) = to_batches(&cols, &rows, 10).unwrap();
        let types: Vec<&DataType> = schema.fields().iter().map(|f| f.data_type()).collect();
        assert_eq!(
            types,
            [
                &DataType::Int64,
                &DataType::Float64,
                &DataType::Boolean,
                &DataType::Utf8,
                &DataType::Utf8,
                &DataType::Utf8
            ]
        );
        assert_eq!(
            schema.field(3).metadata()[platypus_core::META_DB_TYPE],
            "varchar(20)"
        );
        assert_eq!(
            schema.field(1).metadata()[platypus_core::META_DB_TYPE],
            "float"
        );
        let batch = &batches[0];
        assert_eq!(batch.column(0).as_primitive::<Int64Type>().value(1), 2);
        assert!((batch.column(1).as_primitive::<Float64Type>().value(1) - 2.5).abs() < 1e-9);
        assert_eq!(batch.column(4).as_string::<i32>().value(1), "3");
        assert_eq!(batch.column(5).as_string::<i32>().value(0), r#"{"a":1}"#);
        assert!(batch.column(3).is_null(1));
    }

    #[test]
    fn splits_rows_into_batches() {
        let cols = columns(&[("n", None)]);
        let rows: Vec<Vec<Value>> = (0..5).map(|i| vec![json!(i)]).collect();
        let (_, batches) = to_batches(&cols, &rows, 2).unwrap();
        assert_eq!(
            batches
                .iter()
                .map(RecordBatch::num_rows)
                .collect::<Vec<_>>(),
            [2, 2, 1]
        );
        let (_, none) = to_batches(&cols, &[], 2).unwrap();
        assert_eq!(none.len(), 0);
    }

    #[test]
    fn refuses_ragged_rows() {
        let cols = columns(&[("a", None), ("b", None)]);
        assert!(to_batches(&cols, &[vec![json!(1)]], 2).is_err());
    }

    #[test]
    fn keeps_secrets_and_host_options_apart() {
        let mut config = ConnectionConfig::new(DriverKind::Plugin);
        config.host = "db.example.com".to_owned();
        config.password = Some(platypus_core::Secret::new("hunter2"));
        config
            .options
            .insert(PLUGIN_OPTION.to_owned(), "toy".to_owned());
        config
            .options
            .insert("proxy".to_owned(), "http://u:p@proxy:3128".to_owned());
        config.options.insert("region".to_owned(), "eu".to_owned());
        let request = connect_request(&config);
        assert_eq!(request["secrets"]["password"], "hunter2");
        let public = request["config"].to_string();
        assert!(!public.contains("hunter2"), "{public}");
        assert!(!public.contains("proxy"), "{public}");
        assert_eq!(request["config"]["options"]["region"], "eu");
        assert!(request["config"]["options"].get(PLUGIN_OPTION).is_none());
    }
}
