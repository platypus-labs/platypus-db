use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{Error, Result, SessionInit};

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
}

impl DriverKind {
    #[must_use]
    pub const fn default_port(self) -> Option<u16> {
        match self {
            Self::Postgres => Some(5432),
            Self::MySql => Some(3306),
            Self::Sqlite | Self::DuckDb => None,
            Self::SqlServer => Some(1433),
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
        }
    }

    /// Parses a connection URL such as `postgres://user:pass@host:5432/db?sslmode=require`,
    /// `mysql://user@host/db`, `sqlserver://sa@host/master`, `sqlite:///path/to/file.db` or
    /// `duckdb:///path/to/file.duckdb`.
    pub fn from_url(input: &str) -> Result<Self> {
        let url = Url::parse(input).map_err(|e| Error::Config(format!("invalid URL: {e}")))?;
        let driver = match url.scheme() {
            "postgres" | "postgresql" => DriverKind::Postgres,
            "mysql" | "mariadb" => DriverKind::MySql,
            "sqlite" => DriverKind::Sqlite,
            "duckdb" => DriverKind::DuckDb,
            "sqlserver" | "mssql" => DriverKind::SqlServer,
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
                "connect_timeout" => {
                    let secs = value
                        .parse()
                        .map_err(|_| Error::Config(format!("invalid connect_timeout `{value}`")))?;
                    config.connect_timeout = Some(Duration::from_secs(secs));
                }
                other => return Err(Error::Config(format!("unknown URL parameter `{other}`"))),
            }
        }
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
        let c = ConnectionConfig::from_url("mssql://sa@db:14330/").unwrap();
        assert_eq!(c.driver, DriverKind::SqlServer);
        assert_eq!(c.port, Some(14330));
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
