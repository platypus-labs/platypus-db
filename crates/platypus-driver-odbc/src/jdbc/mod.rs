//! Databases reached through a JDBC driver, which the user points at: Db2 for z/OS through IBM's
//! JCC driver and its license jar, or any database that has a JDBC driver. The app has no JVM in
//! it: it starts the bridge (`java/Bridge.java`) with the driver's jars on its class path and
//! talks to it over the standard input and output, the way the ODBC driver talks to its worker
//! thread: the same [`Command`]s, answered here by a thread that owns the bridge process.
//!
//! Requests are lines `<id>\t<COMMAND>\t<argument>…`, each argument the base64 of its UTF-8
//! text. Answers are JSON lines carrying the request's `id`:
//!
//! | Request | Answer |
//! |---|---|
//! | `CONNECT url user password properties timeout` | `{ok, product, version}` |
//! | `EXEC sql batchSize` | `{affected}`, or `{columns}`, then `{batch}` lines, then `{done}` |
//! | `SCHEMAS`, `TABLES schema`, `COLUMNS schema table`, `PRIMARY_KEY …`, `FOREIGN_KEYS …`, `REFERENCING …` | `{rows}` |
//! | `AUTOCOMMIT on`, `COMMIT`, `ROLLBACK` | `{ok}` |
//! | `CANCEL` (any time) | nothing; the running `EXEC` ends with `{done, cancelled}` |
//!
//! Any request may be answered with `{error: {code, vendor, message}}` instead.

mod bridge;
pub mod discover;
mod java;
mod rows;

use std::sync::mpsc as std_mpsc;

use arrow_array::RecordBatch;
use platypus_core::{ConnectionConfig, DriverKind, Error, Result, SslMode};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::command::{CatalogRequest, CatalogRows, Command, Opened, Started};
use crate::meta::{ColumnRow, ForeignKeyRow, KeyRow, TableRow};
use crate::profile;
use bridge::{Bridge, error_of};

/// The version of the JDBC driver the app downloads (IBM's, for Db2) that the connection uses,
/// chosen in the form like a driver of DBeaver's; downloaded when a connection first needs it.
pub const DRIVER: &str = "jdbc.driver";
/// More jars of the JDBC driver (a license jar, or the files of a driver the app does not
/// download), one path per line or separated by `;`. Without `jdbc.driver`, all of them.
pub const JARS: &str = "jdbc.jars";
/// The JDBC URL, for a database other than Db2 (Db2's is made from the host and database).
pub const URL: &str = "jdbc.url";
/// The `java` program, or the Java home, when it is not found by itself.
pub const JAVA: &str = "jdbc.java";
/// More properties for the driver, one `key=value` per line.
pub const PROPERTIES: &str = "jdbc.properties";

/// How many rows of a result wait for the app, in batches.
const BATCHES_AHEAD: usize = 4;

/// The `java` program the bridge would run when the connection names none (`JAVA_HOME`, the
/// `PATH`, DBeaver's runtime), or `None` when there is none.
#[must_use]
pub fn java_program() -> Option<std::path::PathBuf> {
    java::find(None).ok()
}

/// Whether the connection goes through a JDBC driver.
#[must_use]
pub fn enabled(config: &ConnectionConfig) -> bool {
    [DRIVER, JARS]
        .iter()
        .any(|key| profile::option(config, key).is_some_and(|v| !v.trim().is_empty()))
}

/// The jar of the version of IBM's driver the connection names, which must be downloaded.
fn chosen_driver(config: &ConnectionConfig) -> Result<Option<String>> {
    let Some(version) = profile::option(config, DRIVER)
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return Ok(None);
    };
    let driver = platypus_drivers::jdbc_driver("db2jcc", Some(version))
        .ok_or_else(|| Error::Config(format!("there is no IBM JDBC driver version `{version}`")))?;
    let dir = platypus_drivers::data_dir()
        .ok_or_else(|| Error::Config("the app's drivers folder is not set".to_owned()))?;
    platypus_drivers::installed_jdbc(dir, &driver)
        .map(|p| Some(p.to_string_lossy().into_owned()))
        .ok_or_else(|| {
            Error::Config(format!(
                "the IBM JDBC driver {version} is not downloaded: choose it in the connection to \
                 download it"
            ))
        })
}

/// What a bridge needs to connect.
struct Settings {
    java: std::path::PathBuf,
    class_path: String,
    url: String,
    user: String,
    password: String,
    properties: String,
    timeout: String,
}

impl Settings {
    fn new(config: &ConnectionConfig) -> Result<Self> {
        let chosen = chosen_driver(config)?;
        let mut jars: Vec<&str> = chosen.as_deref().into_iter().collect();
        jars.extend(
            profile::option(config, JARS)
                .unwrap_or_default()
                .split([';', '\n'])
                .map(str::trim)
                .filter(|j| !j.is_empty()),
        );
        for jar in &jars {
            if !std::path::Path::new(jar).is_file() {
                return Err(Error::Config(format!(
                    "the JDBC driver file does not exist: {jar}"
                )));
            }
        }
        let (url, mut properties) = match (profile::option(config, URL), config.driver) {
            (Some(url), _) if !url.trim().is_empty() => (url.trim().to_owned(), String::new()),
            (_, DriverKind::Db2) => db2_url(config)?,
            _ => {
                return Err(Error::Config(
                    "a connection through a JDBC driver needs the driver's URL (jdbc:…)".to_owned(),
                ));
            }
        };
        for line in profile::option(config, PROPERTIES)
            .unwrap_or_default()
            .lines()
        {
            properties.push_str(line.trim());
            properties.push('\n');
        }
        Ok(Self {
            java: java::find(profile::option(config, JAVA))?,
            class_path: java::class_path(&java::bridge_jar()?, &jars),
            url,
            user: config.user.clone(),
            password: config
                .password
                .as_ref()
                .map_or_else(String::new, |p| p.expose().to_owned()),
            properties,
            timeout: config
                .connect_timeout
                .map_or_else(String::new, |t| t.as_secs().to_string()),
        })
    }
}

/// IBM's JDBC URL of a Db2 connection, and the properties it takes: TLS where the connection
/// asks for it, and the server's own error messages.
fn db2_url(config: &ConnectionConfig) -> Result<(String, String)> {
    let database = config.database.trim();
    if database.is_empty() || (config.host.trim().is_empty() && config.connect_addr.is_none()) {
        return Err(Error::Config(
            "the Db2 host and database are required".to_owned(),
        ));
    }
    // Through an SSH tunnel: its local end.
    let (host, port) = if let Some(addr) = config.connect_addr {
        let host = match addr.ip() {
            std::net::IpAddr::V6(ip) => format!("[{ip}]"),
            ip @ std::net::IpAddr::V4(_) => ip.to_string(),
        };
        (host, addr.port())
    } else {
        let default = if profile::option(config, profile::DB2_PLATFORM) == Some("zos") {
            446
        } else {
            50000
        };
        (
            config.host.trim().to_owned(),
            config.port.unwrap_or(default),
        )
    };
    let mut properties = String::from("retrieveMessagesFromServerOnGetMessage=true\n");
    if !matches!(config.ssl_mode, SslMode::Disable | SslMode::Prefer) {
        properties.push_str("sslConnection=true\n");
    }
    Ok((format!("jdbc:db2://{host}:{port}/{database}"), properties))
}

/// Starts the worker thread, which starts the bridge and connects.
pub fn spawn(
    config: &ConnectionConfig,
) -> Result<(std_mpsc::Sender<Command>, oneshot::Receiver<Result<Opened>>)> {
    let settings = Settings::new(config)?;
    let (commands, inbox) = std_mpsc::channel();
    let (ready, opened) = oneshot::channel();
    std::thread::Builder::new()
        .name("platypus-jdbc".to_owned())
        .spawn(move || match Worker::open(&settings) {
            Ok((mut worker, info)) => {
                let _ = ready.send(Ok(info));
                // Ends when the connection, and with it the sender, is dropped.
                for command in inbox {
                    worker.run(command);
                }
            }
            Err(e) => {
                let _ = ready.send(Err(e));
            }
        })
        .map_err(|e| Error::Connect(format!("cannot start the JDBC worker: {e}")))?;
    Ok((commands, opened))
}

struct Worker {
    bridge: Bridge,
}

impl Worker {
    fn open(settings: &Settings) -> Result<(Self, Opened)> {
        let mut bridge = Bridge::start(&settings.java, &settings.class_path)?;
        let id = bridge.send(
            "CONNECT",
            &[
                &settings.url,
                &settings.user,
                &settings.password,
                &settings.properties,
                &settings.timeout,
            ],
        )?;
        let answer = answer(&mut bridge, id)?;
        let product = answer["product"].as_str().map(str::to_owned);
        let interrupt = Some(bridge.interrupter());
        Ok((
            Self { bridge },
            Opened {
                dbms: product,
                interrupt,
            },
        ))
    }

    fn run(&mut self, command: Command) {
        match command {
            Command::Execute {
                sql,
                batch_size,
                reply,
            } => self.execute(&sql, batch_size, reply),
            Command::Catalog { request, reply } => {
                let _ = reply.send(self.catalog(&request));
            }
            Command::Autocommit { on, reply } => {
                let _ = reply.send(self.simple("AUTOCOMMIT", &[if on { "true" } else { "false" }]));
            }
            Command::End { commit, reply } => {
                let _ = reply.send(self.simple(if commit { "COMMIT" } else { "ROLLBACK" }, &[]));
            }
        }
    }

    /// A request answered with `ok`.
    fn simple(&mut self, command: &str, args: &[&str]) -> Result<()> {
        let id = self.bridge.send(command, args)?;
        answer(&mut self.bridge, id).map(|_| ())
    }

    fn execute(&mut self, sql: &str, batch_size: usize, reply: oneshot::Sender<Result<Started>>) {
        let first = self
            .bridge
            .send("EXEC", &[sql, &batch_size.to_string()])
            .and_then(|id| answer(&mut self.bridge, id));
        let first = match first {
            Ok(first) => first,
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        };
        if let Some(rows) = first["affected"].as_u64() {
            let _ = reply.send(Ok(Started::Affected(rows)));
            return;
        }
        let columns = first["columns"].as_array().map_or(&[][..], Vec::as_slice);
        let (schema, kinds) = rows::schema(columns);
        let (tx, rx) = mpsc::channel(BATCHES_AHEAD);
        if reply.send(Ok(Started::Rows(schema.clone(), rx))).is_err() {
            // The statement is not wanted: stop it and read to its end.
            self.bridge.cancel();
            self.drain();
            return;
        }
        let mut wanted = true;
        loop {
            let message = match self.bridge.read() {
                Ok(message) => message,
                Err(e) => {
                    let _ = tx.blocking_send(Err(e));
                    return;
                }
            };
            if let Some(e) = error_of(&message) {
                if wanted {
                    let _ = tx.blocking_send(Err(e));
                }
                return;
            }
            if message["done"].as_bool() == Some(true) {
                return;
            }
            let Some(batch) = message["batch"].as_array() else {
                continue;
            };
            if !wanted {
                continue;
            }
            let sent = rows::batch(&schema, &kinds, batch)
                .and_then(|b: RecordBatch| tx.blocking_send(Ok(b)).map_err(|_| Error::Cancelled));
            if sent.is_err() {
                // The app dropped the result: the statement is cancelled and read to its end,
                // so that the next answer is the next request's.
                wanted = false;
                self.bridge.cancel();
            }
        }
    }

    /// Reads and drops answers until the running statement ends.
    fn drain(&mut self) {
        while let Ok(message) = self.bridge.read() {
            if message["done"].as_bool() == Some(true) || message.get("error").is_some() {
                return;
            }
        }
    }

    fn catalog(&mut self, request: &CatalogRequest) -> Result<CatalogRows> {
        let (command, args): (&str, Vec<&str>) = match request {
            CatalogRequest::Schemas => ("SCHEMAS", vec![]),
            CatalogRequest::Tables { schema } => ("TABLES", vec![schema.as_deref().unwrap_or("")]),
            CatalogRequest::Columns { schema, table } => {
                ("COLUMNS", vec![schema, table.as_deref().unwrap_or("")])
            }
            CatalogRequest::PrimaryKey { schema, table } => ("PRIMARY_KEY", vec![schema, table]),
            CatalogRequest::ForeignKeys { schema, table } => ("FOREIGN_KEYS", vec![schema, table]),
            CatalogRequest::Referencing { schema, table } => ("REFERENCING", vec![schema, table]),
        };
        let id = self.bridge.send(command, &args)?;
        let answer = answer(&mut self.bridge, id)?;
        let rows = answer["rows"].as_array().map_or(&[][..], Vec::as_slice);
        Ok(match request {
            CatalogRequest::Schemas => {
                CatalogRows::Schemas(rows.iter().filter_map(|r| text(r, 0)).collect())
            }
            CatalogRequest::Tables { .. } => {
                CatalogRows::Tables(rows.iter().map(table_row).collect())
            }
            CatalogRequest::Columns { .. } => {
                CatalogRows::Columns(rows.iter().map(column_row).collect())
            }
            CatalogRequest::PrimaryKey { .. } => {
                CatalogRows::Keys(rows.iter().map(key_row).collect())
            }
            CatalogRequest::ForeignKeys { .. } | CatalogRequest::Referencing { .. } => {
                CatalogRows::ForeignKeys(rows.iter().map(foreign_key_row).collect())
            }
        })
    }
}

/// The answer to request `id`: its first message, or the error it is.
fn answer(bridge: &mut Bridge, id: u64) -> Result<Value> {
    loop {
        let message = bridge.read()?;
        if message["id"].as_u64() != Some(id) {
            continue;
        }
        return match error_of(&message) {
            Some(e) => Err(e),
            None => Ok(message),
        };
    }
}

fn text(row: &Value, i: usize) -> Option<String> {
    row[i].as_str().map(str::to_owned)
}

fn int(row: &Value, i: usize) -> Option<i64> {
    row[i].as_i64()
}

fn table_row(row: &Value) -> TableRow {
    // What a JDBC driver calls a plain table varies: `BASE TABLE`, `TABLE`.
    let kind = text(row, 2).unwrap_or_default();
    TableRow {
        schema: text(row, 0).unwrap_or_default(),
        name: text(row, 1).unwrap_or_default(),
        kind: if kind.eq_ignore_ascii_case("BASE TABLE") {
            "TABLE".to_owned()
        } else {
            kind
        },
        remarks: text(row, 3).filter(|r| !r.is_empty()),
    }
}

fn column_row(row: &Value) -> ColumnRow {
    ColumnRow {
        schema: text(row, 0).unwrap_or_default(),
        table: text(row, 1).unwrap_or_default(),
        name: text(row, 2).unwrap_or_default(),
        sql_type: int(row, 3)
            .and_then(|t| i16::try_from(t).ok())
            .unwrap_or_default(),
        type_name: text(row, 4).unwrap_or_default(),
        size: int(row, 5).and_then(|s| i32::try_from(s).ok()),
        digits: int(row, 6).and_then(|d| i16::try_from(d).ok()),
        // `columnNoNulls` is 0; 1 is nullable and 2 unknown.
        nullable: int(row, 7) != Some(0),
        default: text(row, 8),
        remarks: text(row, 9).filter(|r| !r.is_empty()),
    }
}

fn key_row(row: &Value) -> KeyRow {
    KeyRow {
        column: text(row, 0).unwrap_or_default(),
        seq: int(row, 1)
            .and_then(|s| i16::try_from(s).ok())
            .unwrap_or_default(),
        name: text(row, 2),
    }
}

fn foreign_key_row(row: &Value) -> ForeignKeyRow {
    ForeignKeyRow {
        pk_schema: text(row, 0).unwrap_or_default(),
        pk_table: text(row, 1).unwrap_or_default(),
        pk_column: text(row, 2).unwrap_or_default(),
        fk_schema: text(row, 3).unwrap_or_default(),
        fk_table: text(row, 4).unwrap_or_default(),
        fk_column: text(row, 5).unwrap_or_default(),
        seq: int(row, 6)
            .and_then(|s| i16::try_from(s).ok())
            .unwrap_or_default(),
        name: text(row, 7),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn config(driver: DriverKind) -> ConnectionConfig {
        ConnectionConfig::new(driver)
    }

    #[test]
    fn the_jars_turn_the_bridge_on() {
        let mut c = config(DriverKind::Db2);
        assert!(!enabled(&c));
        c.options.insert(JARS.to_owned(), "  ".to_owned());
        assert!(!enabled(&c));
        c.options.insert(JARS.to_owned(), "C:\\jcc.jar".to_owned());
        assert!(enabled(&c));
    }

    #[test]
    fn a_db2_url_is_made_from_the_host_and_the_platform_port() {
        let mut c = config(DriverKind::Db2);
        c.host = "db2m.example.com".to_owned();
        c.database = "CAR_DB2M".to_owned();
        // A new Db2 connection holds its driver's port: the platform's is for an empty one.
        c.port = None;
        c.options
            .insert(profile::DB2_PLATFORM.to_owned(), "zos".to_owned());
        let (url, properties) = db2_url(&c).unwrap();
        assert_eq!(url, "jdbc:db2://db2m.example.com:446/CAR_DB2M");
        assert!(properties.contains("retrieveMessagesFromServerOnGetMessage=true"));
        assert!(!properties.contains("sslConnection"));
        c.port = Some(50443);
        c.ssl_mode = SslMode::Require;
        let (url, properties) = db2_url(&c).unwrap();
        assert!(url.ends_with(":50443/CAR_DB2M"));
        assert!(properties.contains("sslConnection=true"));
    }

    #[test]
    fn catalog_rows_follow_the_odbc_shapes() {
        let table = table_row(&json!(["PUBLIC", "T", "BASE TABLE", ""]));
        assert_eq!((table.kind.as_str(), table.remarks), ("TABLE", None));
        let column = column_row(&json!(["S", "T", "A", 4, "INTEGER", 32, 0, 0, null, null]));
        assert_eq!(column.sql_type, 4);
        assert!(!column.nullable);
        assert!(column_row(&json!(["S", "T", "B", 12, "VARCHAR", 20, 0, 1, null, null])).nullable);
    }
}
