use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::net::{Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{Error, Result, SessionInit};

mod any;
mod jdbc;
mod keyvalue;
mod libpq;
mod to_url;

pub use any::{ParseNote, ParsedConnection};
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
    /// Oracle's Instant Client, IBM's ODBC driver, DuckDB's library. `None` for the generic
    /// ODBC driver, which uses what the system has.
    #[must_use]
    pub const fn vendor_package(self) -> Option<&'static str> {
        match self {
            Self::Oracle => Some("oracle"),
            Self::Db2 => Some("db2"),
            Self::DuckDb => Some("duckdb"),
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

impl SslMode {
    /// The libpq name, as `sslmode=` takes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Prefer => "prefer",
            Self::Require => "require",
            Self::VerifyCa => "verify-ca",
            Self::VerifyFull => "verify-full",
        }
    }
}

/// The libpq names, in any case, and the names MySQL (`REQUIRED`, `VERIFY_IDENTITY`) and
/// Npgsql (`VerifyFull`) give the same modes.
impl FromStr for SslMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let name: String = s
            .chars()
            .filter(|c| !matches!(c, '-' | '_'))
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match name.as_str() {
            "disable" | "disabled" | "none" => Ok(Self::Disable),
            "prefer" | "preferred" | "allow" => Ok(Self::Prefer),
            "require" | "required" => Ok(Self::Require),
            "verifyca" => Ok(Self::VerifyCa),
            "verifyfull" | "verifyidentity" => Ok(Self::VerifyFull),
            _ => Err(Error::Config(format!("unknown sslmode `{s}`"))),
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

/// The option that says a pooler stands between the app and a PostgreSQL server, and its
/// mode: `transaction` (statements run unnamed: see the driver) or `session`.
pub const POOLER: &str = "pooler";

/// The options with the shell commands run before connecting and after the connection
/// closes (run by `platypus-net`). Their `${VAR}` belong to the shell: they are not replaced.
pub const BEFORE_CONNECT: &str = "before_connect";
pub const AFTER_DISCONNECT: &str = "after_disconnect";

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
    /// MongoDB connection string (`mongodb://`, `mongodb+srv://`). An unknown parameter is
    /// an error; [`ConnectionConfig::parse_any`] leaves it out with a note instead.
    pub fn from_url(input: &str) -> Result<Self> {
        let read = Self::read_url(input, None)?;
        if let Some(host) = read.other_hosts.first() {
            return Err(Error::Config(format!(
                "the URL names several hosts; only one is supported (`{host}` is another)"
            )));
        }
        match read.unknown.first() {
            Some(key) => Err(Error::Config(format!("unknown URL parameter `{key}`"))),
            None => Ok(read.config),
        }
    }

    /// The TLS mode of a URL that does not say: `libsql://` is HTTPS (Turso's URLs give the
    /// token, which goes to the server only over verified TLS); the other schemes follow
    /// [`SslMode::default`].
    #[must_use]
    pub const fn url_ssl_mode(driver: DriverKind) -> SslMode {
        match driver {
            DriverKind::Libsql => SslMode::VerifyFull,
            _ => SslMode::Prefer,
        }
    }

    /// A URL as [`ConnectionConfig::from_url`] reads it, with the parameters it does not
    /// know. `hint` is the database the URL is pasted for: it decides what `http://` and
    /// `https://` (libSQL or ClickHouse) are.
    fn read_url(input: &str, hint: Option<DriverKind>) -> Result<UrlRead> {
        let mongo = match input.strip_prefix("mongodb+srv://") {
            Some(rest) => Some(Self::from_mongo_url(rest, true)?),
            None => input
                .strip_prefix("mongodb://")
                .map(|rest| Self::from_mongo_url(rest, false))
                .transpose()?,
        };
        if let Some(config) = mongo {
            config.check_host()?;
            return Ok(UrlRead::of(config));
        }
        // libpq's failover list (`h1:5432,h2:5433`), which `Url` refuses: the first host.
        let (input, other_hosts) = first_host(input);
        // An IPv6 zone (`[fe80::1%25eth0]`, RFC 6874), which `Url` refuses: put back after.
        let (input, zone) = take_zone(&input)?;
        let url = Url::parse(&input).map_err(|e| Error::Config(format!("invalid URL: {e}")))?;
        let driver = match url.scheme() {
            "postgres" | "postgresql" => DriverKind::Postgres,
            "mysql" | "mariadb" => DriverKind::MySql,
            "sqlite" => DriverKind::Sqlite,
            "duckdb" => DriverKind::DuckDb,
            "sqlserver" | "mssql" => DriverKind::SqlServer,
            "clickhouse" => DriverKind::ClickHouse,
            // libSQL clients take Hrana over WebSocket too; the app speaks HTTP to the same
            // server.
            "libsql" | "ws" | "wss" => DriverKind::Libsql,
            "http" | "https"
                if matches!(hint, Some(DriverKind::Libsql | DriverKind::ClickHouse)) =>
            {
                hint.unwrap_or(DriverKind::Libsql)
            }
            other => return Err(Error::Config(format!("unsupported URL scheme `{other}`"))),
        };
        let mut config = Self::new(driver);
        config.ssl_mode = match url.scheme() {
            "ws" | "http" => SslMode::Disable,
            "wss" | "https" => SslMode::VerifyFull,
            _ => Self::url_ssl_mode(driver),
        };

        if driver.is_embedded() {
            config.host.clear();
            config.database = embedded_path(&url, &input)?;
            return Ok(UrlRead::of(config));
        }

        if let Some(host) = url.host_str() {
            config.host = url_host(host)?;
            if let Some(zone) = zone {
                let _ = write!(config.host, "%{zone}");
            }
        }
        if let Some(port) = url.port() {
            config.port = Some(port);
        }
        config.database = decode(url.path().trim_start_matches('/'))?;
        config.user = decode(url.username())?;
        config.password = url.password().map(decode).transpose()?.map(Secret::new);

        let unknown = config.read_query(&url)?;
        // ClickHouse serves HTTPS on another port than HTTP.
        if driver == DriverKind::ClickHouse
            && url.port().is_none()
            && !matches!(config.ssl_mode, SslMode::Disable | SslMode::Prefer)
        {
            config.port = Some(8443);
        }
        config.check_host()?;
        Ok(UrlRead {
            config,
            unknown,
            other_hosts,
        })
    }

    /// The query parameters of a URL into the settings; the names of those it does not know.
    /// SQL Server takes `database`, `encrypt` and `trustServerCertificate` as its JDBC and
    /// ADO.NET strings do, libSQL `tls=0`/`tls=1` as its clients do, MySQL `sslMode`.
    fn read_query(&mut self, url: &Url) -> Result<Vec<String>> {
        let mut unknown = Vec::new();
        let mut ssl_given = false;
        let (mut encrypt, mut trust) = (None, false);
        let sqlserver = self.driver == DriverKind::SqlServer;
        for (key, value) in url.query_pairs() {
            match &*key {
                "sslmode" | "ssl-mode" | "sslMode" | "ssl_mode" => {
                    self.ssl_mode = value.parse()?;
                    ssl_given = true;
                }
                "sslrootcert" => self.ssl_root_cert = Some(PathBuf::from(&*value)),
                "sslcert" => self.ssl_client_cert = Some(PathBuf::from(&*value)),
                "sslkey" => self.ssl_client_key = Some(PathBuf::from(&*value)),
                "application_name" => self.application_name = Some(value.into_owned()),
                // Prisma's and Supabase's flag for a pooler in transaction mode.
                "pgbouncer" if self.driver == DriverKind::Postgres => {
                    if matches!(value.to_ascii_lowercase().as_str(), "true" | "1" | "yes") {
                        self.options
                            .insert(POOLER.to_owned(), "transaction".to_owned());
                    }
                }
                // The query parameter of Turso's URLs.
                "authToken" => self.password = Some(Secret::new(value)),
                "connect_timeout" => {
                    let secs = value
                        .parse()
                        .map_err(|_| Error::Config(format!("invalid connect_timeout `{value}`")))?;
                    self.connect_timeout = Some(Duration::from_secs(secs));
                }
                "tls" if self.driver == DriverKind::Libsql => {
                    self.ssl_mode = match value.to_ascii_lowercase().as_str() {
                        "0" | "false" | "no" => SslMode::Disable,
                        "1" | "true" | "yes" => SslMode::VerifyFull,
                        _ => return Err(Error::Config(format!("invalid tls `{value}`"))),
                    };
                    ssl_given = true;
                }
                k if sqlserver
                    && (k.eq_ignore_ascii_case("database")
                        || k.eq_ignore_ascii_case("databaseName")) =>
                {
                    self.database = value.into_owned();
                }
                k if sqlserver && k.eq_ignore_ascii_case("encrypt") => {
                    encrypt = jdbc::sqlserver_flag(&value);
                }
                k if sqlserver && k.eq_ignore_ascii_case("trustServerCertificate") => {
                    trust = jdbc::sqlserver_flag(&value) == Some(true);
                }
                other => unknown.push(other.to_owned()),
            }
        }
        if !ssl_given && let Some(mode) = jdbc::sqlserver_ssl(encrypt, trust) {
            self.ssl_mode = mode;
        }
        Ok(unknown)
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
                // An IPv6 zone's `%` is written `%25`.
                config.host = decode(host.trim_matches(['[', ']']))?;
                config.port = Some(port);
            }
            // A seed list keeps the brackets of its IPv6 hosts.
            None if hosts.contains(',') => hosts.clone_into(&mut config.host),
            None => config.host = decode(hosts.trim_matches(['[', ']']))?,
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
        // The path's database is where the user authenticates, whether the password is in
        // the URL or typed apart (the connection string spec, mongosh).
        if !database.is_empty()
            && !config.user.is_empty()
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

impl ConnectionConfig {
    /// Refuses a host that cannot be one, as a URL or connection string gives when it puts
    /// a login or other keys where the host goes: one with `@`, `=`, `;`, spaces, or a `:`
    /// outside an IPv6 address. A MongoDB seed list (`a:27017,b:27018`) is checked host by
    /// host. The error does not repeat the host, which may hold a password.
    pub fn check_host(&self) -> Result<()> {
        let valid = if self.driver == DriverKind::Mongo {
            self.host.split(',').all(seed_host)
        } else {
            plain_host(&self.host)
        };
        if valid {
            Ok(())
        } else {
            Err(Error::Config(
                "the host is not valid: it holds `@`, `=`, `;`, spaces or a `:` that is not \
                 part of an IPv6 address"
                    .to_owned(),
            ))
        }
    }

    /// Takes the options that carry a secret out of a MongoDB connection's `database`,
    /// where [`ConnectionConfig::from_url`] keeps the options, and returns their names.
    fn drop_secret_options(&mut self) -> Vec<String> {
        let Some((database, query)) = self.database.split_once('?') else {
            return Vec::new();
        };
        let (secret, kept): (Vec<&str>, Vec<&str>) = query
            .split('&')
            .filter(|p| !p.is_empty())
            .partition(|p| secret_mongo_option(p));
        let names = secret
            .iter()
            .map(|p| p.split('=').next().unwrap_or_default().to_owned())
            .collect();
        let rest = if kept.is_empty() {
            database.to_owned()
        } else {
            format!("{database}?{}", kept.join("&"))
        };
        self.database = rest;
        names
    }
}

/// What a URL gives: the connection, the parameters it does not know, and the hosts after
/// the first of a failover list.
pub(crate) struct UrlRead {
    pub config: ConnectionConfig,
    pub unknown: Vec<String>,
    pub other_hosts: Vec<String>,
}

impl UrlRead {
    const fn of(config: ConnectionConfig) -> Self {
        Self {
            config,
            unknown: Vec::new(),
            other_hosts: Vec::new(),
        }
    }
}

/// The URL with only the first host of a `host1:port1,host2:port2` list, and the others.
fn first_host(input: &str) -> (std::borrow::Cow<'_, str>, Vec<String>) {
    let Some((scheme, rest)) = input.split_once("://") else {
        return (input.into(), Vec::new());
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let (login, hosts) = match authority.rsplit_once('@') {
        Some((login, hosts)) => (Some(login), hosts),
        None => (None, authority),
    };
    let mut list = hosts.split(',');
    let first = list.next().unwrap_or_default();
    let others: Vec<String> = list.filter(|h| !h.is_empty()).map(str::to_owned).collect();
    if others.is_empty() {
        return (input.into(), others);
    }
    let login = login.map(|l| format!("{l}@")).unwrap_or_default();
    (format!("{scheme}://{login}{first}{tail}").into(), others)
}

/// The URL without the zone of its IPv6 host (`[fe80::1%25eth0]`, or `%eth0` as typed), and
/// the zone, decoded.
fn take_zone(input: &str) -> Result<(std::borrow::Cow<'_, str>, Option<String>)> {
    let Some((scheme, rest)) = input.split_once("://") else {
        return Ok((input.into(), None));
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let start = authority.rfind('@').map_or(0, |at| at + 1);
    let host = &authority[start..];
    let Some(close) = host.find(']').filter(|_| host.starts_with('[')) else {
        return Ok((input.into(), None));
    };
    let Some(percent) = host[..close].find('%') else {
        return Ok((input.into(), None));
    };
    let raw = &host[percent + 1..close];
    let zone = decode(rfc_zone(raw).unwrap_or(raw))?;
    if zone.is_empty() {
        return Err(Error::Config(
            "the IPv6 zone of the URL is empty".to_owned(),
        ));
    }
    let cut = scheme.len() + 3 + start;
    let out = format!(
        "{}{}{}",
        &input[..cut + percent],
        &host[close..],
        &rest[start + host.len()..]
    );
    Ok((out.into(), Some(zone)))
}

/// The zone after `%25`, RFC 6874's escaped `%`, when `raw` is one: not when what follows
/// is no zone, empty or a number with a leading zero, so that the interfaces typed as
/// they are named (`%25`, `%250`) keep their number.
fn rfc_zone(raw: &str) -> Option<&str> {
    raw.strip_prefix("25").filter(|zone| {
        let leading_zero = zone.starts_with('0') && zone.bytes().all(|b| b.is_ascii_digit());
        !(zone.is_empty() || leading_zero)
    })
}

/// The file of a SQLite or DuckDB URL. `sqlite:///C:/data/app.db` has the path
/// `/C:/data/app.db`; `sqlite://C:/data/app.db`, with two slashes, has the drive as its
/// host. `duckdb:///:memory:` is the in-memory database, as `to_url` writes it.
fn embedded_path(url: &Url, input: &str) -> Result<String> {
    let path = decode(url.path())?;
    let host = url.host_str().unwrap_or_default();
    let drive = |s: &str| s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic();
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        // `C:` reads as the host `C` without a port.
        let written_as_drive = input
            .split_once("://")
            .is_some_and(|(_, rest)| rest.starts_with(&format!("{host}:")));
        if drive(host) && url.port().is_none() && written_as_drive {
            return Ok(format!("{host}:{path}"));
        }
        return Err(Error::Config(format!(
            "the file of a {} URL goes after three slashes, as in {}:///C:/data/app.db",
            url.scheme(),
            url.scheme()
        )));
    }
    if path == "/:memory:" {
        return Ok(":memory:".to_owned());
    }
    let windows_drive =
        path.len() > 2 && path.as_bytes()[2] == b':' && path.as_bytes()[1].is_ascii_alphabetic();
    Ok(if windows_drive {
        path[1..].to_owned()
    } else {
        path
    })
}

/// A URL's host as a resolver takes it: without IPv6 brackets, and a name with non-ASCII
/// letters (`dé.example`, percent-encoded in a URL of a scheme `Url` does not know) as its
/// punycode.
fn url_host(host: &str) -> Result<String> {
    let host = decode(host.trim_matches(['[', ']']))?;
    if host.is_ascii() {
        return Ok(host);
    }
    match url::Host::parse(&host) {
        Ok(url::Host::Domain(ascii)) => Ok(ascii),
        _ => Err(Error::Config(
            "the host name of the URL is not valid".to_owned(),
        )),
    }
}

/// A host name, an IPv4 address or an IPv6 one (with its zone, if any), without a port.
fn plain_host(host: &str) -> bool {
    if host
        .chars()
        .any(|c| matches!(c, '@' | '=' | ';' | '[' | ']') || c.is_whitespace() || c.is_control())
    {
        return false;
    }
    let address = host.split_once('%').map_or(host, |(a, _)| a);
    !host.contains(':') || address.parse::<Ipv6Addr>().is_ok()
}

/// One host of a MongoDB seed list: `host`, `host:port`, `[v6]` or `[v6]:port`.
fn seed_host(entry: &str) -> bool {
    let (host, port) = if let Some(inner) = entry.strip_prefix('[') {
        let Some((host, rest)) = inner.split_once(']') else {
            return false;
        };
        if !rest.is_empty() && !rest.starts_with(':') {
            return false;
        }
        (host, rest.strip_prefix(':'))
    } else {
        match entry.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') => (h, Some(p)),
            _ => (entry, None),
        }
    };
    !host.is_empty() && plain_host(host) && port.is_none_or(|p| p.parse::<u16>().is_ok())
}

/// Whether an option's name says it holds a secret: `password`, `PrivateKeyPwd`,
/// `tlsCertificateKeyFilePassword`, `token`, `client_secret`, …
fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "pwd",
        "passcode",
        "passphrase",
        "secret",
        "token",
        "apikey",
        "api_key",
    ]
    .iter()
    .any(|word| key.contains(word))
}

/// Whether a MongoDB option (`key=value`) carries a secret: by its name, or as
/// `authMechanismProperties` holding one (`AWS_SESSION_TOKEN:…`).
fn secret_mongo_option(pair: &str) -> bool {
    let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
    if secret_key(key) {
        return true;
    }
    let value = decode(value).unwrap_or_else(|_| value.to_owned());
    key.eq_ignore_ascii_case("authMechanismProperties")
        && value
            .split(',')
            .any(|p| secret_key(p.split(':').next().unwrap_or_default()))
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
        assert_eq!(DriverKind::DuckDb.vendor_package(), Some("duckdb"));
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
    fn reads_and_writes_the_pooler_flag() {
        let url =
            "postgres://postgres.abc@aws-0-eu.pooler.supabase.com:6543/postgres?pgbouncer=true";
        let c = ConnectionConfig::from_url(url).unwrap();
        assert_eq!(
            c.options.get(POOLER).map(String::as_str),
            Some("transaction")
        );
        assert_eq!(c.to_url().as_deref(), Some(url));
        let c = ConnectionConfig::from_url("postgres://h/db?pgbouncer=false").unwrap();
        assert_eq!(c.options.get(POOLER), None);
        assert!(ConnectionConfig::from_url("mysql://h/db?pgbouncer=true").is_err());
    }

    #[test]
    fn reads_the_query_keys_of_each_engine() {
        let ms = ConnectionConfig::from_url(
            "sqlserver://sa@localhost:51433?database=master&encrypt=true&trustServerCertificate=true",
        )
        .unwrap();
        assert_eq!(ms.database, "master");
        assert_eq!(ms.ssl_mode, SslMode::Require);
        let ms = ConnectionConfig::from_url("sqlserver://sa@h?databaseName=x&Encrypt=yes").unwrap();
        assert_eq!(
            (ms.database.as_str(), ms.ssl_mode),
            ("x", SslMode::VerifyFull)
        );
        let ms = ConnectionConfig::from_url("sqlserver://sa@h?encrypt=false").unwrap();
        assert_eq!(ms.ssl_mode, SslMode::Disable);
        // An explicit mode wins over `encrypt`.
        let ms = ConnectionConfig::from_url("sqlserver://h?encrypt=true&sslmode=disable").unwrap();
        assert_eq!(ms.ssl_mode, SslMode::Disable);
        // Other engines do not take SQL Server's keys.
        assert!(ConnectionConfig::from_url("postgres://h/d?database=x").is_err());

        let my = ConnectionConfig::from_url("mysql://app@h:3306/shop?sslMode=REQUIRED").unwrap();
        assert_eq!(my.ssl_mode, SslMode::Require);

        let lib = ConnectionConfig::from_url("libsql://localhost:58080?tls=0&authToken=t").unwrap();
        assert_eq!(lib.ssl_mode, SslMode::Disable);
        assert_eq!(lib.password, Some(Secret::new("t")));
        let lib = ConnectionConfig::from_url("libsql://h?tls=1").unwrap();
        assert_eq!(lib.ssl_mode, SslMode::VerifyFull);
        assert!(ConnectionConfig::from_url("libsql://h?tls=maybe").is_err());
        assert!(ConnectionConfig::from_url("postgres://h/d?tls=0").is_err());
    }

    #[test]
    fn reads_ipv6_zones() {
        for url in [
            "postgres://u@[fe80::1%25eth0]:5432/db",
            "postgres://u@[fe80::1%eth0]:5432/db",
        ] {
            let c = ConnectionConfig::from_url(url).unwrap();
            assert_eq!(c.host, "fe80::1%eth0", "{url}");
            assert_eq!(
                (c.port, c.database.as_str(), c.user.as_str()),
                (Some(5432), "db", "u")
            );
        }
        let c = ConnectionConfig::from_url("mysql://[fe80::1%2512]/x").unwrap();
        assert_eq!(c.host, "fe80::1%12");
        // Interfaces numbered as typed: no zone could follow an escaped `%`.
        for (url, host) in [
            ("postgres://[fe80::1%250]/db", "fe80::1%250"),
            ("postgres://[fe80::1%25]/db", "fe80::1%25"),
            ("postgres://[fe80::1%2501]/db", "fe80::1%2501"),
        ] {
            assert_eq!(ConnectionConfig::from_url(url).unwrap().host, host, "{url}");
        }
        assert!(ConnectionConfig::from_url("postgres://[fe80::1%]/db").is_err());
        assert!(ConnectionConfig::from_url("postgres://u@db.example:5432x/shop").is_err());
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
    fn refuses_hosts_that_hold_a_login_or_keys() {
        let host = |driver, host: &str| {
            let mut c = ConnectionConfig::new(driver);
            host.clone_into(&mut c.host);
            c.check_host()
        };
        for ok in [
            "db.example.com",
            "10.0.0.5",
            "::1",
            "fe80::1%eth0",
            "/tmp",
            "",
        ] {
            assert!(host(DriverKind::Postgres, ok).is_ok(), "{ok}");
        }
        for bad in [
            "sandy:hunter2@myhost",
            "h port=5433 password=hunter2",
            "a;b",
            "h:hunter2",
            "[::1]",
        ] {
            let err = host(DriverKind::Postgres, bad).unwrap_err().to_string();
            assert!(!err.contains("hunter2"), "{err}");
        }
        for ok in [
            "a:1,b:2",
            "[::1]:27017,[::2]",
            "cluster0.example.net",
            "::1",
        ] {
            assert!(host(DriverKind::Mongo, ok).is_ok(), "{ok}");
        }
        for bad in ["a:x,b", "a,,b", "u:p@h:1,b"] {
            assert!(host(DriverKind::Mongo, bad).is_err(), "{bad}");
        }
        let c = ConnectionConfig::from_url("mongodb://[::1]:1,[::2]:2/db").unwrap();
        assert_eq!(c.host, "[::1]:1,[::2]:2");
    }

    #[test]
    fn drops_secret_mongo_options() {
        let mut c = ConnectionConfig::from_url(
            "mongodb://u@h/db?replicaSet=rs0&tlsCertificateKeyFilePassword=hunter2\
             &authMechanismProperties=AWS_SESSION_TOKEN%3Ahunter2&w=majority",
        )
        .unwrap();
        assert_eq!(
            c.drop_secret_options(),
            ["tlsCertificateKeyFilePassword", "authMechanismProperties"]
        );
        assert_eq!(c.database, "db?replicaSet=rs0&w=majority&authSource=db");
        let mut c = ConnectionConfig::from_url(
            "mongodb://u@h/db?authMechanismProperties=SERVICE_NAME:mongodb",
        )
        .unwrap();
        assert_eq!(c.drop_secret_options(), Vec::<String>::new());
    }

    #[test]
    fn mongo_auth_source_does_not_depend_on_the_password_in_the_url() {
        for url in ["mongodb://u:p@h:27017/shop", "mongodb://u@h:27017/shop"] {
            let c = ConnectionConfig::from_url(url).unwrap();
            assert_eq!(c.database, "shop?authSource=shop", "{url}");
        }
        // No user, nothing to authenticate; an explicit authSource is kept.
        let c = ConnectionConfig::from_url("mongodb://h/shop").unwrap();
        assert_eq!(c.database, "shop");
        let c = ConnectionConfig::from_url("mongodb://u@h/shop?authSource=admin").unwrap();
        assert_eq!(c.database, "shop?authSource=admin");
    }

    #[test]
    fn libsql_urls_verify_tls_unless_they_say_otherwise() {
        let c = ConnectionConfig::from_url("libsql://db-acme.turso.io?authToken=tok").unwrap();
        assert_eq!(c.ssl_mode, SslMode::VerifyFull);
        let c = ConnectionConfig::from_url("libsql://h?sslmode=require").unwrap();
        assert_eq!(c.ssl_mode, SslMode::Require);
        let c = ConnectionConfig::from_url("ws://localhost:8080").unwrap();
        assert_eq!(
            (c.driver, c.ssl_mode),
            (DriverKind::Libsql, SslMode::Disable)
        );
        assert_eq!(c.port, Some(8080));
        let c = ConnectionConfig::from_url("wss://db-acme.turso.io").unwrap();
        assert_eq!(
            (c.driver, c.ssl_mode),
            (DriverKind::Libsql, SslMode::VerifyFull)
        );
        // `http(s)://` only for a form that says which database it is.
        assert!(ConnectionConfig::from_url("http://localhost:8080").is_err());
        let p = ConnectionConfig::parse_any_for("http://localhost:8080", Some(DriverKind::Libsql))
            .unwrap();
        assert_eq!(
            (p.config.driver, p.config.ssl_mode, p.config.port),
            (DriverKind::Libsql, SslMode::Disable, Some(8080))
        );
        let p = ConnectionConfig::parse_any_for("https://ch.example", Some(DriverKind::ClickHouse))
            .unwrap();
        assert_eq!(
            (p.config.driver, p.config.ssl_mode, p.config.port),
            (DriverKind::ClickHouse, SslMode::VerifyFull, Some(8443))
        );
        // Other databases keep `prefer` as their default.
        let c = ConnectionConfig::from_url("postgres://u@h/d").unwrap();
        assert_eq!(c.ssl_mode, SslMode::Prefer);
    }

    #[test]
    fn embedded_urls_keep_drives_and_the_in_memory_database() {
        let file = |url: &str| ConnectionConfig::from_url(url).map(|c| c.database);
        assert_eq!(file("sqlite://C:/data/x.db").unwrap(), "C:/data/x.db");
        assert_eq!(file("sqlite:///C:/data/x.db").unwrap(), "C:/data/x.db");
        assert_eq!(file("sqlite://localhost/tmp/x.db").unwrap(), "/tmp/x.db");
        assert_eq!(file("duckdb:///:memory:").unwrap(), ":memory:");
        assert_eq!(file("duckdb::memory:").unwrap(), ":memory:");
        // A host that is no drive: the user meant a path, which needs three slashes.
        let err = file("sqlite://data/x.db").unwrap_err().to_string();
        assert!(err.contains("three slashes"), "{err}");
        assert!(file("sqlite://C:5/x.db").is_err());
    }

    #[test]
    fn non_ascii_hosts_become_punycode() {
        let c = ConnectionConfig::from_url("postgres://u@dé.example/db").unwrap();
        assert_eq!(c.host, "xn--d-bga.example");
        let c = ConnectionConfig::from_url("mysql://u@d%C3%A9.example/db").unwrap();
        assert_eq!(c.host, "xn--d-bga.example");
        let c = ConnectionConfig::from_url("postgres://u@Db.Example/db").unwrap();
        assert_eq!(c.host, "Db.Example");
    }

    #[test]
    fn failover_lists_keep_the_first_host() {
        let p = ConnectionConfig::parse_any("postgres://u@h1:5432,h2:5433/db").unwrap();
        assert_eq!((p.config.host.as_str(), p.config.port), ("h1", Some(5432)));
        assert_eq!(p.config.database, "db");
        assert_eq!(
            p.notes,
            [ParseNote::OtherHosts {
                hosts: vec!["h2:5433".to_owned()]
            }]
        );
        let err = ConnectionConfig::from_url("postgres://u:hunter2@h1,h2/db")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("several hosts") && !err.contains("hunter2"),
            "{err}"
        );
    }

    #[test]
    fn hosts_are_checked_for_logins() {
        let mut c = ConnectionConfig::new(DriverKind::MySql);
        c.host = "root:s3cret@db.example.com:3306".to_owned();
        let err = c.check_host().unwrap_err().to_string();
        assert!(!err.contains("s3cret"), "{err}");
    }

    #[test]
    fn secret_is_not_printed() {
        let c = ConnectionConfig::from_url("postgres://u:hunter2@h/d").unwrap();
        assert!(!format!("{c:?}").contains("hunter2"));
    }
}
