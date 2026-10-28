//! A connection written back as the URL [`ConnectionConfig::from_url`] reads, without its
//! password, to share or paste elsewhere.

use std::fmt::Write as _;
use std::path::Path;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use super::{ConnectionConfig, DriverKind, SslMode, secret_mongo_option};

/// What a user name, database or parameter value keeps as it is: RFC 3986's unreserved
/// characters.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// A file path: its separators and drive colon stay readable.
const PATH: &AsciiSet = &COMPONENT.remove(b'/').remove(b':');

fn encode(s: &str) -> String {
    utf8_percent_encode(s, COMPONENT).to_string()
}

impl ConnectionConfig {
    /// The URL of the connection without its password (nor a token, for libSQL):
    /// `postgres://user@host:5432/db?sslmode=require`, `sqlite:///C:/data/app.db`,
    /// `mongodb+srv://user@cluster/db?…`. `None` for the drivers that have no URL scheme
    /// (the ADBC and ODBC ones, Oracle).
    #[must_use]
    pub fn to_url(&self) -> Option<String> {
        let scheme = match self.driver {
            DriverKind::Postgres => "postgres",
            DriverKind::MySql => "mysql",
            DriverKind::SqlServer => "sqlserver",
            DriverKind::ClickHouse => "clickhouse",
            DriverKind::Libsql => "libsql",
            DriverKind::Sqlite => "sqlite",
            DriverKind::DuckDb => "duckdb",
            DriverKind::Mongo => return Some(self.mongo_url()),
            _ => return None,
        };
        if self.driver.is_embedded() {
            let path = utf8_percent_encode(self.database.trim_start_matches('/'), PATH);
            return Some(format!("{scheme}:///{path}"));
        }
        let mut url = format!("{scheme}://{}", self.authority(self.port));
        // A libSQL server has no database to name.
        if self.driver != DriverKind::Libsql || !self.database.is_empty() {
            url.push('/');
            url.push_str(&encode(&self.database));
        }
        let mut params = Vec::new();
        if self.ssl_mode != Self::url_ssl_mode(self.driver) {
            params.push(format!("sslmode={}", self.ssl_mode.as_str()));
        }
        for (key, path) in [
            ("sslrootcert", &self.ssl_root_cert),
            ("sslcert", &self.ssl_client_cert),
            ("sslkey", &self.ssl_client_key),
        ] {
            if let Some(path) = path {
                params.push(format!("{key}={}", encode_path(path)));
            }
        }
        if !params.is_empty() {
            let _ = write!(url, "?{}", params.join("&"));
        }
        Some(url)
    }

    /// `user@host:port`, the host in brackets when it is an IPv6 address, its zone's `%`
    /// encoded (`[fe80::1%25eth0]`, RFC 6874).
    fn authority(&self, port: Option<u16>) -> String {
        let mut out = String::new();
        if !self.user.is_empty() {
            let _ = write!(out, "{}@", encode(&self.user));
        }
        if self.host.contains(':') && !self.host.contains(',') {
            let _ = write!(out, "[{}]", self.host.replacen('%', "%25", 1));
        } else {
            out.push_str(&self.host);
        }
        if let Some(port) = port {
            let _ = write!(out, ":{port}");
        }
        out
    }

    /// A MongoDB connection string: the options kept after the database go back to the
    /// query, but for those that hold a secret, `srv=true` to the scheme, and the TLS mode
    /// to `tls=`.
    fn mongo_url(&self) -> String {
        let (database, query) = self
            .database
            .split_once('?')
            .unwrap_or((self.database.as_str(), ""));
        let mut params: Vec<String> = query
            .split('&')
            .filter(|p| !p.is_empty() && !secret_mongo_option(p))
            .map(str::to_owned)
            .collect();
        let srv = params.iter().any(|p| p == "srv=true");
        params.retain(|p| p != "srv=true");
        match self.ssl_mode {
            SslMode::Prefer => {}
            SslMode::Disable => params.push("tls=false".to_owned()),
            _ => params.push("tls=true".to_owned()),
        }
        if let Some(ca) = &self.ssl_root_cert {
            params.push(format!("tlsCAFile={}", encode_path(ca)));
        }
        // A seed list, or an SRV name, has no port of its own.
        let port = self.port.filter(|_| !srv && !self.host.contains(','));
        let scheme = if srv { "mongodb+srv" } else { "mongodb" };
        let mut url = format!("{scheme}://{}/{}", self.authority(port), encode(database));
        if !params.is_empty() {
            let _ = write!(url, "?{}", params.join("&"));
        }
        url
    }
}

fn encode_path(path: &Path) -> String {
    utf8_percent_encode(&path.to_string_lossy(), PATH).to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::Secret;

    /// Read back, the URL gives the same connection, but for the password.
    fn round_trip(url: &str) -> String {
        let mut config = ConnectionConfig::from_url(url).unwrap();
        config.password = None;
        let out = config.to_url().unwrap();
        assert_eq!(ConnectionConfig::from_url(&out).unwrap(), config, "{out}");
        out
    }

    #[test]
    fn urls_round_trip_without_the_password() {
        assert_eq!(
            round_trip("postgres://us%40er:p%3Ass@db.example.com:6543/my%20db?sslmode=require"),
            "postgres://us%40er@db.example.com:6543/my%20db?sslmode=require"
        );
        assert_eq!(
            round_trip("mysql://root@localhost/app"),
            "mysql://root@localhost:3306/app"
        );
        assert_eq!(
            round_trip("sqlserver://sa:pw@db/sales?sslmode=verify-full"),
            "sqlserver://sa@db:1433/sales?sslmode=verify-full"
        );
        assert_eq!(
            round_trip("clickhouse://ch/logs?sslmode=require"),
            "clickhouse://ch:8443/logs?sslmode=require"
        );
        assert_eq!(
            round_trip("libsql://db-acme.turso.io?authToken=tok"),
            "libsql://db-acme.turso.io"
        );
        assert_eq!(
            round_trip("postgres://u@[::1]:5432/db"),
            "postgres://u@[::1]:5432/db"
        );
        assert_eq!(
            round_trip("postgres://u@[fe80::1%25eth0]:5432/db"),
            "postgres://u@[fe80::1%25eth0]:5432/db"
        );
        assert_eq!(
            round_trip("sqlite:///C:/data/my%20app.db"),
            "sqlite:///C:/data/my%20app.db"
        );
        assert_eq!(
            round_trip("duckdb:///tmp/x.duckdb"),
            "duckdb:///tmp/x.duckdb"
        );
        round_trip("postgres://u@h/d?sslmode=verify-ca&sslrootcert=C:/certs/ca%20root.pem");
        // libSQL's default is verified TLS; another mode is written out.
        assert_eq!(
            round_trip("libsql://h?sslmode=disable"),
            "libsql://h?sslmode=disable"
        );
        assert_eq!(
            round_trip("libsql://h?sslmode=prefer"),
            "libsql://h?sslmode=prefer"
        );
    }

    #[test]
    fn ipv6_zones_print_encoded() {
        let mut config = ConnectionConfig::new(DriverKind::Postgres);
        config.host = "fe80::1%eth0".to_owned();
        let url = config.to_url().unwrap();
        assert_eq!(url, "postgres://[fe80::1%25eth0]:5432/");
        assert_eq!(ConnectionConfig::from_url(&url).unwrap().host, config.host);
        config.driver = DriverKind::Mongo;
        config.port = Some(27017);
        let url = config.to_url().unwrap();
        assert_eq!(url, "mongodb://[fe80::1%25eth0]:27017/");
        assert_eq!(ConnectionConfig::from_url(&url).unwrap().host, config.host);
    }

    #[test]
    fn query_settings_print_as_sslmode() {
        for (url, printed) in [
            (
                "sqlserver://sa@h:51433?database=master&encrypt=true&trustServerCertificate=true",
                "sqlserver://sa@h:51433/master?sslmode=require",
            ),
            (
                "mysql://app@h:3306/shop?sslMode=REQUIRED",
                "mysql://app@h:3306/shop?sslmode=require",
            ),
            ("libsql://h:8080?tls=0", "libsql://h:8080?sslmode=disable"),
        ] {
            let config = ConnectionConfig::from_url(url).unwrap();
            assert_eq!(config.to_url().unwrap(), printed);
        }
    }

    #[test]
    fn in_memory_and_drive_urls_round_trip() {
        let mut duck = ConnectionConfig::new(DriverKind::DuckDb);
        duck.host.clear();
        duck.database = ":memory:".to_owned();
        let url = duck.to_url().unwrap();
        assert_eq!(
            ConnectionConfig::from_url(&url).unwrap().database,
            ":memory:"
        );
        assert_eq!(
            round_trip("sqlite://C:/data/x.db"),
            "sqlite:///C:/data/x.db"
        );
    }

    #[test]
    fn mongo_urls_round_trip() {
        assert_eq!(
            round_trip("mongodb://us%40er@db:27018/shop?replicaSet=rs0&tls=true"),
            "mongodb://us%40er@db:27018/shop?replicaSet=rs0&authSource=shop&tls=true"
        );
        assert_eq!(
            round_trip("mongodb+srv://u@cluster0.example.net/app"),
            "mongodb+srv://u@cluster0.example.net/app?authSource=app"
        );
        assert_eq!(
            round_trip("mongodb://a:1,b:2/?authSource=admin"),
            "mongodb://a:1,b:2/?authSource=admin"
        );
    }

    #[test]
    fn mongo_urls_leave_out_secret_options() {
        let config = ConnectionConfig::from_url(
            "mongodb://u:p@h:27017/db?tls=true&tlsCertificateKeyFilePassword=hunter2\
             &authMechanismProperties=AWS_SESSION_TOKEN:hunter2&appSecret=hunter2&w=1",
        )
        .unwrap();
        let url = config.to_url().unwrap();
        assert!(!url.contains("hunter2"), "{url}");
        assert_eq!(url, "mongodb://u@h:27017/db?w=1&authSource=db&tls=true");
    }

    #[test]
    fn never_writes_the_password_and_skips_drivers_without_a_scheme() {
        let mut config = ConnectionConfig::from_url("postgres://u:hunter2@h/d").unwrap();
        assert!(config.password.is_some());
        assert!(!config.to_url().unwrap().contains("hunter2"));
        config.password = Some(Secret::new("hunter2"));
        config.driver = DriverKind::Libsql;
        assert!(!config.to_url().unwrap().contains("hunter2"));
        config.driver = DriverKind::Oracle;
        assert_eq!(config.to_url(), None);
        let mut sqlite = ConnectionConfig::new(DriverKind::Sqlite);
        sqlite.database = r"C:\data\app.db".to_owned();
        let url = sqlite.to_url().unwrap();
        assert_eq!(url, "sqlite:///C:%5Cdata%5Capp.db");
        assert_eq!(
            ConnectionConfig::from_url(&url).unwrap().database,
            sqlite.database
        );
        sqlite.ssl_root_cert = Some(PathBuf::from("x"));
        assert_eq!(sqlite.to_url().unwrap(), url);
    }
}
