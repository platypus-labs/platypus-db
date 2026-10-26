use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{Error, Result, SessionInit};

mod jdbc;

pub use jdbc::JdbcUrl;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriverKind {
    Postgres,
    MySql,
    Sqlite,
    /// Embedded like SQLite: `database` is the file path, or `:memory:`.
    DuckDb,
    /// Microsoft SQL Server and Azure SQL.
    SqlServer,
    /// Over HTTP: port 8123, or 8443 with TLS.
    ClickHouse,
    /// A libSQL server (`sqld`, Turso) over HTTP; the password is the auth token. Without a
    /// port, the scheme's default (443, or 80 with `sslmode=disable`).
    Libsql,
    /// MongoDB: queries in mongosh syntax instead of SQL. `host` may list several
    /// `host:port` pairs separated by commas, and `database` may end with the options of a
    /// connection string (`shop?authSource=admin&replicaSet=rs0`; `srv=true` for
    /// `mongodb+srv`).
    Mongo,
    /// Snowflake through its ADBC driver: `host` is the account, `options` hold
    /// `warehouse`, `role` and `schema`.
    Snowflake,
    /// Google `BigQuery` through its ADBC driver: `options` hold `project`, `dataset` and
    /// `credentials_file` (empty for Application Default Credentials).
    #[serde(rename = "bigquery")]
    BigQuery,
    /// Databricks through its ADBC driver: `host` is the workspace, `database` the catalog,
    /// `options` hold `http_path` and `schema`; the password is the access token.
    Databricks,
    /// Trino through its ADBC driver: `database` is the catalog, `options` hold `schema`.
    Trino,
    /// Any ADBC driver: `options` hold `driver_file`, `uri` and the driver's own options.
    Adbc,
    /// Oracle Database through its native client library (Oracle Instant Client, downloaded
    /// by the app): `database` is the service name, or a net service name without a host.
    Oracle,
    /// IBM Db2 through IBM's ODBC (CLI) driver.
    Db2,
    /// Any ODBC driver or data source: `options` hold `dsn`, `driver` or
    /// `connection_string`.
    Odbc,
}

/// How the app reaches a database whose driver is not built in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bridge {
    /// An ADBC driver library, loaded at run time.
    Adbc,
    /// An ODBC driver registered with the system's driver manager.
    Odbc,
}

impl DriverKind {
    #[must_use]
    pub const fn default_port(self) -> Option<u16> {
        match self {
            Self::Postgres => Some(5432),
            Self::MySql => Some(3306),
            Self::SqlServer => Some(1433),
            Self::ClickHouse => Some(8123),
            Self::Mongo => Some(27017),
            Self::Trino => Some(8080),
            Self::Oracle => Some(1521),
            Self::Db2 => Some(50000),
            Self::Sqlite
            | Self::DuckDb
            | Self::Libsql
            | Self::Snowflake
            | Self::BigQuery
            | Self::Databricks
            | Self::Adbc
            | Self::Odbc => None,
        }
    }

    /// The bridge to the driver of a database that is not built in.
    #[must_use]
    pub const fn bridge(self) -> Option<Bridge> {
        match self {
            Self::Snowflake | Self::BigQuery | Self::Databricks | Self::Trino | Self::Adbc => {
                Some(Bridge::Adbc)
            }
            Self::Db2 | Self::Odbc => Some(Bridge::Odbc),
            _ => None,
        }
    }

    /// The product's name, for messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Postgres => "PostgreSQL",
            Self::MySql => "MySQL",
            Self::Sqlite => "SQLite",
            Self::DuckDb => "DuckDB",
            Self::SqlServer => "SQL Server",
            Self::ClickHouse => "ClickHouse",
            Self::Libsql => "libSQL",
            Self::Mongo => "MongoDB",
            Self::Snowflake => "Snowflake",
            Self::BigQuery => "BigQuery",
            Self::Databricks => "Databricks",
            Self::Trino => "Trino",
            Self::Adbc => "ADBC",
            Self::Oracle => "Oracle",
            Self::Db2 => "Db2",
            Self::Odbc => "ODBC",
        }
    }

    /// The name of the ADBC driver package in the driver registry; `None` for built-in
    /// drivers and for the generic ADBC driver, whose file the user picks.
    #[must_use]
    pub const fn adbc_package(self) -> Option<&'static str> {
        match self {
            Self::Snowflake => Some("snowflake"),
            Self::BigQuery => Some("bigquery"),
            Self::Databricks => Some("databricks"),
            Self::Trino => Some("trino"),
            _ => None,
        }
    }

    /// The name of the vendor's client library the app downloads for this database:
    /// Oracle's Instant Client, IBM's ODBC driver. `None` for the generic ODBC driver,
    /// which uses what the system has.
    #[must_use]
    pub const fn vendor_package(self) -> Option<&'static str> {
        match self {
            Self::Oracle => Some("oracle"),
            Self::Db2 => Some("db2"),
            _ => None,
        }
    }

    /// Runs in-process on a local file (SQLite, DuckDB): no server, host, user or password.
    #[must_use]
    pub const fn is_embedded(self) -> bool {
        matches!(self, Self::Sqlite | Self::DuckDb)
    }
}

/// TLS behaviour, following libpq `sslmode` semantics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SslMode {
    Disable,
    /// Use TLS when the server offers it, fall back to plaintext otherwise.
    #[default]
    Prefer,
    /// Require TLS without verifying the server certificate.
    Require,
    /// Require TLS and verify the certificate chain, but not the host name.
    VerifyCa,
    /// Require TLS and verify both certificate chain and host name.
    VerifyFull,
}

impl FromStr for SslMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "disable" => Ok(Self::Disable),
            "prefer" | "allow" => Ok(Self::Prefer),
            "require" => Ok(Self::Require),
            "verify-ca" => Ok(Self::VerifyCa),
            "verify-full" => Ok(Self::VerifyFull),
            other => Err(Error::Config(format!("unknown sslmode `{other}`"))),
        }
    }
}

/// A password or token. Never printed by `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// How to log in to an SSH server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshAuth {
    Password(Secret),
    /// A private key file (OpenSSH or PEM), with its passphrase if it has one.
    Key {
        path: PathBuf,
        passphrase: Option<Secret>,
    },
    /// The keys of the running SSH agent: OpenSSH's or Pageant on Windows, `SSH_AUTH_SOCK`
    /// elsewhere.
    Agent,
}

/// An SSH server the connection goes through, possibly reached through another one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: SshAuth,
    /// A jump host to reach this server through (`ssh -J`).
    pub jump: Option<Box<SshConfig>>,
}

/// Everything a driver needs to open a connection.
///
/// For SQLite and DuckDB, `database` is the file path and the network fields are ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    pub driver: DriverKind,
    pub host: String,
    pub port: Option<u16>,
    pub database: String,
    pub user: String,
    pub password: Option<Secret>,
    pub ssl_mode: SslMode,
    /// PEM file with the CA certificates to trust; the OS trust store is used when `None`.
    pub ssl_root_cert: Option<PathBuf>,
    /// PEM certificate (chain) and private key the client presents, for servers that
    /// authenticate clients by certificate. Both or neither.
    pub ssl_client_cert: Option<PathBuf>,
    pub ssl_client_key: Option<PathBuf>,
    pub application_name: Option<String>,
    pub connect_timeout: Option<Duration>,
    /// Where to open the TCP connection when it is not `host:port`, e.g. the local end of an
    /// SSH tunnel; `host` still names the server for TLS.
    pub connect_addr: Option<SocketAddr>,
    /// The SSH tunnel to reach the server through. Drivers ignore it: the app opens the
    /// tunnel and sets `connect_addr`.
    pub ssh: Option<SshConfig>,
    /// Default schema, time zone and bootstrap statements, run by the driver after connecting.
    pub init: SessionInit,
    /// What a driver needs beyond the fields above, e.g. Snowflake's `warehouse`.
    pub options: BTreeMap<String, String>,
}

impl ConnectionConfig {
    #[must_use]
    pub fn new(driver: DriverKind) -> Self {
        Self {
            driver,
            host: "localhost".to_owned(),
            port: driver.default_port(),
            database: String::new(),
            user: String::new(),
            password: None,
            ssl_mode: SslMode::default(),
            ssl_root_cert: None,
            ssl_client_cert: None,
            ssl_client_key: None,
            application_name: Some("PlatypusDB".to_owned()),
            connect_timeout: Some(Duration::from_secs(10)),
            connect_addr: None,
            ssh: None,
            init: SessionInit::default(),
            options: BTreeMap::new(),
        }
    }

    /// Parses a connection URL such as `postgres://user:pass@host:5432/db?sslmode=require`,
    /// `mysql://user@host/db`, `sqlserver://sa@host/master`, `sqlite:///path/to/file.db`,
    /// `duckdb:///path/to/file.duckdb` or `libsql://db-org.turso.io?authToken=...`, or a
    /// MongoDB connection string (`mongodb://`, `mongodb+srv://`).
    pub fn from_url(input: &str) -> Result<Self> {
        if let Some(rest) = input.strip_prefix("mongodb+srv://") {
            return Self::from_mongo_url(rest, true);
        }
        if let Some(rest) = input.strip_prefix("mongodb://") {
            return Self::from_mongo_url(rest, false);
        }
        let url = Url::parse(input).map_err(|e| Error::Config(format!("invalid URL: {e}")))?;
        let driver = match url.scheme() {
            "postgres" | "postgresql" => DriverKind::Postgres,
            "mysql" | "mariadb" => DriverKind::MySql,
            "sqlite" => DriverKind::Sqlite,
            "duckdb" => DriverKind::DuckDb,
            "sqlserver" | "mssql" => DriverKind::SqlServer,
            "clickhouse" => DriverKind::ClickHouse,
            "libsql" => DriverKind::Libsql,
            other => return Err(Error::Config(format!("unsupported URL scheme `{other}`"))),
        };
        let mut config = Self::new(driver);

        if driver.is_embedded() {
            config.host.clear();
            let path = decode(url.path())?;
            // `sqlite:///C:/data/app.db` has the path `/C:/data/app.db`.
            let windows_drive = path.len() > 2
                && path.as_bytes()[2] == b':'
                && path.as_bytes()[1].is_ascii_alphabetic();
            config.database = if windows_drive {
                path[1..].to_owned()
            } else {
                path
            };
            return Ok(config);
        }

        if let Some(host) = url.host_str() {
            host.trim_matches(['[', ']']).clone_into(&mut config.host);
        }
        if let Some(port) = url.port() {
            config.port = Some(port);
        }
        config.database = decode(url.path().trim_start_matches('/'))?;
        config.user = decode(url.username())?;
        config.password = url.password().map(decode).transpose()?.map(Secret::new);

        for (key, value) in url.query_pairs() {
            match &*key {
                "sslmode" | "ssl-mode" => config.ssl_mode = value.parse()?,
                "sslrootcert" => config.ssl_root_cert = Some(PathBuf::from(&*value)),
                "sslcert" => config.ssl_client_cert = Some(PathBuf::from(&*value)),
                "sslkey" => config.ssl_client_key = Some(PathBuf::from(&*value)),
                "application_name" => config.application_name = Some(value.into_owned()),
                // The query parameter of Turso's URLs.
                "authToken" => config.password = Some(Secret::new(value)),
                "connect_timeout" => {
                    let secs = value
                        .parse()
                        .map_err(|_| Error::Config(format!("invalid connect_timeout `{value}`")))?;
                    config.connect_timeout = Some(Duration::from_secs(secs));
                }
                other => return Err(Error::Config(format!("unknown URL parameter `{other}`"))),
            }
        }
        // ClickHouse serves HTTPS on another port than HTTP.
        if driver == DriverKind::ClickHouse
            && url.port().is_none()
            && !matches!(config.ssl_mode, SslMode::Disable | SslMode::Prefer)
        {
            config.port = Some(8443);
        }
        Ok(config)
    }

    /// A MongoDB connection string after its scheme. It can list several hosts, which
    /// [`Url`] does not parse; the options go to `database` for the driver, along with
    /// `authSource` set to the path's database as the connection string means it.
    fn from_mongo_url(rest: &str, srv: bool) -> Result<Self> {
        let mut config = Self::new(DriverKind::Mongo);
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        let hosts = match authority.rsplit_once('@') {
            Some((userinfo, hosts)) => {
                let (user, password) = userinfo.split_once(':').unwrap_or((userinfo, ""));
                config.user = decode(user)?;
                if userinfo.contains(':') {
                    config.password = Some(Secret::new(decode(password)?));
                }
                hosts
            }
            None => authority,
        };
        if hosts.is_empty() {
            return Err(Error::Config("the MongoDB URL has no host".to_owned()));
        }
        let single_port = hosts
            .rsplit_once(':')
            .filter(|(host, _)| !hosts.contains(',') && !host.ends_with(':'))
            .and_then(|(host, port)| Some((host, port.parse::<u16>().ok()?)));
        match single_port {
            Some((host, port)) => {
                host.trim_matches(['[', ']']).clone_into(&mut config.host);
                config.port = Some(port);
            }
            None => hosts.trim_matches(['[', ']']).clone_into(&mut config.host),
        }
        let database = decode(path)?;
        let mut options: Vec<String> = Vec::new();
        if srv {
            options.push("srv=true".to_owned());
        }
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            match key {
                "appName" => config.application_name = Some(decode(value)?),
                "connectTimeoutMS" => {
                    let ms = value.parse().map_err(|_| {
                        Error::Config(format!("invalid connectTimeoutMS `{value}`"))
                    })?;
                    config.connect_timeout = Some(Duration::from_millis(ms));
                }
                "tls" | "ssl" => {
                    config.ssl_mode = if value == "true" {
                        SslMode::VerifyFull
                    } else {
                        SslMode::Disable
                    };
                }
                "tlsCAFile" => config.ssl_root_cert = Some(PathBuf::from(decode(value)?)),
                _ => options.push(pair.to_owned()),
            }
        }
        if !database.is_empty()
            && config.password.is_some()
            && !query.split('&').any(|p| p.starts_with("authSource="))
        {
            options.push(format!("authSource={database}"));
        }
        config.database = if options.is_empty() {
            database
        } else {
            format!("{database}?{}", options.join("&"))
        };
        Ok(config)
    }
}

fn decode(s: &str) -> Result<String> {
    percent_encoding::percent_decode_str(s)
        .decode_utf8()
        .map(Into::into)
        .map_err(|e| Error::Config(format!("invalid percent-encoding: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_kinds_round_trip() {
        for (kind, name) in [
            (DriverKind::Snowflake, "snowflake"),
            (DriverKind::BigQuery, "bigquery"),
            (DriverKind::Databricks, "databricks"),
            (DriverKind::Trino, "trino"),
            (DriverKind::Adbc, "adbc"),
            (DriverKind::Oracle, "oracle"),
            (DriverKind::Db2, "db2"),
            (DriverKind::Odbc, "odbc"),
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{name}\""));
            assert_eq!(serde_json::from_str::<DriverKind>(&json).unwrap(), kind);
        }
    }

    #[test]
    fn bridge_and_packages() {
        assert_eq!(DriverKind::Postgres.bridge(), None);
        assert_eq!(DriverKind::Postgres.adbc_package(), None);
        assert_eq!(DriverKind::Snowflake.bridge(), Some(Bridge::Adbc));
        assert_eq!(DriverKind::Snowflake.adbc_package(), Some("snowflake"));
        assert_eq!(DriverKind::BigQuery.adbc_package(), Some("bigquery"));
        assert_eq!(DriverKind::Databricks.adbc_package(), Some("databricks"));
        assert_eq!(DriverKind::Trino.adbc_package(), Some("trino"));
        // The generic driver takes the file the user picks.
        assert_eq!(DriverKind::Adbc.bridge(), Some(Bridge::Adbc));
        assert_eq!(DriverKind::Adbc.adbc_package(), None);
        assert_eq!(DriverKind::Trino.default_port(), Some(8080));
        assert_eq!(DriverKind::Snowflake.default_port(), None);
        assert!(!DriverKind::Trino.is_embedded());
        for kind in [DriverKind::Db2, DriverKind::Odbc] {
            assert_eq!(kind.bridge(), Some(Bridge::Odbc));
            assert_eq!(kind.adbc_package(), None);
        }
        // Native, with a client library downloaded by the app.
        assert_eq!(DriverKind::Oracle.bridge(), None);
        assert_eq!(DriverKind::Oracle.vendor_package(), Some("oracle"));
        assert_eq!(DriverKind::Db2.vendor_package(), Some("db2"));
        assert_eq!(DriverKind::Odbc.vendor_package(), None);
        assert_eq!(DriverKind::Snowflake.vendor_package(), None);
        assert_eq!(DriverKind::Oracle.default_port(), Some(1521));
        assert_eq!(DriverKind::Db2.default_port(), Some(50000));
        assert_eq!(DriverKind::Odbc.default_port(), None);
        assert!(
            Error::DriverMissing {
                driver: DriverKind::Trino
            }
            .to_string()
            .contains("not installed")
        );
    }

    #[test]
    fn parses_postgres_url() {
        let c = ConnectionConfig::from_url(
            "postgres://us%40er:p%3Ass@db.example.com:6543/my%20db?sslmode=require&application_name=x",
        )
        .unwrap();
        assert_eq!(c.driver, DriverKind::Postgres);
        assert_eq!(c.host, "db.example.com");
        assert_eq!(c.port, Some(6543));
        assert_eq!(c.database, "my db");
        assert_eq!(c.user, "us@er");
        assert_eq!(c.password, Some(Secret::new("p:ss")));
        assert_eq!(c.ssl_mode, SslMode::Require);
        assert_eq!(c.application_name.as_deref(), Some("x"));
    }

    #[test]
    fn defaults_port_and_ssl() {
        let c = ConnectionConfig::from_url("mysql://root@localhost/app").unwrap();
        assert_eq!(c.driver, DriverKind::MySql);
        assert_eq!(c.port, Some(3306));
        assert_eq!(c.ssl_mode, SslMode::Prefer);
        assert_eq!(c.password, None);
    }

    #[test]
    fn parses_sql_server_url() {
        let c = ConnectionConfig::from_url("sqlserver://sa:pw@db/sales?sslmode=require").unwrap();
        assert_eq!(c.driver, DriverKind::SqlServer);
        assert_eq!(c.port, Some(1433));
        assert_eq!(c.database, "sales");
        assert_eq!(c.ssl_mode, SslMode::Require);
        let c = ConnectionConfig::from_url("clickhouse://default@ch/logs").unwrap();
        assert_eq!(c.driver, DriverKind::ClickHouse);
        assert_eq!((c.port, c.database.as_str()), (Some(8123), "logs"));
        let c = ConnectionConfig::from_url("clickhouse://ch/logs?sslmode=require").unwrap();
        assert_eq!(c.port, Some(8443));
        let c = ConnectionConfig::from_url("clickhouse://ch:9443/?sslmode=verify-full").unwrap();
        assert_eq!(c.port, Some(9443));
        let c = ConnectionConfig::from_url("mssql://sa@db:14330/").unwrap();
        assert_eq!(c.driver, DriverKind::SqlServer);
        assert_eq!(c.port, Some(14330));
    }

    #[test]
    fn parses_libsql_url() {
        let c = ConnectionConfig::from_url("libsql://db-acme.turso.io?authToken=tok").unwrap();
        assert_eq!(c.driver, DriverKind::Libsql);
        assert_eq!((c.host.as_str(), c.port), ("db-acme.turso.io", None));
        assert_eq!(c.password, Some(Secret::new("tok")));
        let c = ConnectionConfig::from_url("libsql://:tok@localhost:8080?sslmode=disable").unwrap();
        assert_eq!((c.port, c.ssl_mode), (Some(8080), SslMode::Disable));
        assert_eq!(c.password, Some(Secret::new("tok")));
    }

    #[test]
    fn parses_mongodb_url() {
        let c = ConnectionConfig::from_url(
            "mongodb://us%40er:p%3Ass@db:27018/shop?replicaSet=rs0&tls=true",
        )
        .unwrap();
        assert_eq!(c.driver, DriverKind::Mongo);
        assert_eq!((c.host.as_str(), c.port), ("db", Some(27018)));
        assert_eq!(c.user, "us@er");
        assert_eq!(c.password, Some(Secret::new("p:ss")));
        assert_eq!(c.ssl_mode, SslMode::VerifyFull);
        assert_eq!(c.database, "shop?replicaSet=rs0&authSource=shop");

        let c = ConnectionConfig::from_url("mongodb://a:1,b:2/?authSource=admin").unwrap();
        assert_eq!((c.host.as_str(), c.port), ("a:1,b:2", Some(27017)));
        assert_eq!(c.database, "?authSource=admin");

        let c = ConnectionConfig::from_url("mongodb+srv://u:p@cluster0.example.net/app").unwrap();
        assert_eq!(c.host, "cluster0.example.net");
        assert_eq!(c.database, "app?srv=true&authSource=app");
        assert!(ConnectionConfig::from_url("mongodb:///db").is_err());
    }

    #[test]
    fn parses_ipv6_host() {
        let c = ConnectionConfig::from_url("postgres://u@[::1]:5432/db").unwrap();
        assert_eq!(c.host, "::1");
    }

    #[test]
    fn parses_sqlite_path() {
        let c = ConnectionConfig::from_url("sqlite:///tmp/test.db").unwrap();
        assert_eq!(c.driver, DriverKind::Sqlite);
        assert_eq!(c.database, "/tmp/test.db");
        let c = ConnectionConfig::from_url("sqlite:///C:/data/my%20app.db").unwrap();
        assert_eq!(c.database, "C:/data/my app.db");
        let c = ConnectionConfig::from_url("duckdb:///C:/data/sales.duckdb").unwrap();
        assert_eq!(c.driver, DriverKind::DuckDb);
        assert_eq!(
            (c.database.as_str(), c.port),
            ("C:/data/sales.duckdb", None)
        );
    }

    #[test]
    fn rejects_unknown_scheme_and_params() {
        assert!(ConnectionConfig::from_url("oracle://x/y").is_err());
        assert!(ConnectionConfig::from_url("postgres://x/y?foo=1").is_err());
        assert!(ConnectionConfig::from_url("postgres://x/y?sslmode=nope").is_err());
    }

    #[test]
    fn secret_is_not_printed() {
        let c = ConnectionConfig::from_url("postgres://u:hunter2@h/d").unwrap();
        assert!(!format!("{c:?}").contains("hunter2"));
    }
}
