//! JDBC URLs, as Java tools (DBeaver, DataGrip, …) keep them: PostgreSQL, MySQL/MariaDB,
//! SQL Server (Microsoft's driver, jTDS and the old `jdbc:microsoft:` one), Oracle thin and
//! OCI, ClickHouse, SQLite and DuckDB.

use std::collections::BTreeMap;
use std::fmt;

use super::{ConnectionConfig, DriverKind, Secret, SslMode, decode, secret_key};
use crate::{Error, Result};

/// A JDBC URL read into a [`ConnectionConfig`], with what the config has no field for.
/// `Debug` hides the values of the properties that hold a secret.
#[derive(Clone, PartialEq, Eq)]
pub struct JdbcUrl {
    pub config: ConnectionConfig,
    /// Whether the URL names the port; `config.port` is the driver's default otherwise.
    pub port_given: bool,
    /// A SQL Server named instance (`host\SQLEXPRESS`, `instanceName=`). The app connects
    /// to a port, not through the SQL Server Browser, so without a port it needs one.
    pub instance: Option<String>,
    /// The URL's properties (`;key=value` or `?key=value&…`), keys in lower case.
    pub properties: BTreeMap<String, String>,
    /// The hosts after the first of a failover list (`//h1:5432,h2:5432/db`): the app
    /// connects to the first one only.
    pub other_hosts: Vec<String>,
}

impl fmt::Debug for JdbcUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let properties: BTreeMap<&str, &str> = self
            .properties
            .iter()
            .map(|(k, v)| (k.as_str(), if secret_key(k) { "***" } else { v.as_str() }))
            .collect();
        f.debug_struct("JdbcUrl")
            .field("config", &self.config)
            .field("port_given", &self.port_given)
            .field("instance", &self.instance)
            .field("properties", &properties)
            .field("other_hosts", &self.other_hosts)
            .finish()
    }
}

impl ConnectionConfig {
    /// Parses a JDBC URL such as `jdbc:sqlserver://host\instance:1433;databaseName=x`,
    /// `jdbc:postgresql://host/db?sslmode=require`, `jdbc:mysql://host:3307/db` or
    /// `jdbc:oracle:thin:@host:1521/service`. See [`JdbcUrl`] for what it does not keep.
    pub fn from_jdbc_url(input: &str) -> Result<Self> {
        JdbcUrl::parse(input).map(|jdbc| jdbc.config)
    }
}

impl JdbcUrl {
    /// Reads a JDBC URL of one of the databases above; an error names only the
    /// subprotocol of another one, since the URL may hold a password. A host that holds
    /// a login or other keys is an error, never a host.
    pub fn parse(input: &str) -> Result<Self> {
        let jdbc = Self::read(input.trim())?;
        jdbc.config.check_host()?;
        Ok(jdbc)
    }

    fn read(input: &str) -> Result<Self> {
        let Some(rest) = input.strip_prefix("jdbc:") else {
            return Err(Error::Config("a JDBC URL starts with `jdbc:`".to_owned()));
        };
        for (prefix, driver) in [
            ("sqlite:", DriverKind::Sqlite),
            ("duckdb:", DriverKind::DuckDb),
        ] {
            if let Some(path) = rest.strip_prefix(prefix) {
                let mut jdbc = Self::new(driver);
                jdbc.config.host.clear();
                jdbc.config.database = if path.is_empty() && driver == DriverKind::DuckDb {
                    ":memory:".to_owned()
                } else {
                    path.to_owned()
                };
                return Ok(jdbc);
            }
        }
        for prefix in [
            "sqlserver://",
            "jtds:sqlserver://",
            "microsoft:sqlserver://",
        ] {
            if let Some(r) = rest.strip_prefix(prefix) {
                return Ok(Self::sqlserver(r, prefix.starts_with("jtds")));
            }
        }
        for prefix in ["oracle:thin:", "oracle:oci:", "oracle:oci8:"] {
            if let Some(r) = rest.strip_prefix(prefix) {
                return Ok(Self::oracle(r.strip_prefix('@').unwrap_or(r)));
            }
        }
        for (prefix, driver) in [
            ("postgresql:", DriverKind::Postgres),
            ("mysql:", DriverKind::MySql),
            ("mariadb:", DriverKind::MySql),
            ("clickhouse:", DriverKind::ClickHouse),
            ("ch:", DriverKind::ClickHouse),
        ] {
            if let Some(r) = rest.strip_prefix(prefix) {
                return Self::hierarchical(driver, r);
            }
        }
        // The subprotocol only: the rest may hold a password.
        let kind = rest
            .split([':', '/', ';', '?', '@'])
            .next()
            .unwrap_or_default();
        Err(Error::Config(format!(
            "unsupported JDBC URL `jdbc:{kind}:`"
        )))
    }

    /// A SQL Server host as `srv\SQLEXPRESS`: the host and the instance.
    #[must_use]
    pub fn split_instance(host: &str) -> (String, Option<String>) {
        match host.split_once('\\') {
            Some((h, i)) => (
                h.trim().to_owned(),
                Some(i.trim().to_owned()).filter(|i| !i.is_empty()),
            ),
            None => (host.to_owned(), None),
        }
    }

    fn new(driver: DriverKind) -> Self {
        Self {
            config: ConnectionConfig::new(driver),
            port_given: false,
            instance: None,
            properties: BTreeMap::new(),
            other_hosts: Vec::new(),
        }
    }

    fn set_address(&mut self, address: &str) {
        let (host, port) = host_port(address);
        if !host.is_empty() {
            self.config.host = host;
        }
        if let Some(port) = port {
            self.config.port = Some(port);
            self.port_given = true;
        }
    }

    fn property(&self, keys: &[&str]) -> Option<String> {
        keys.iter().find_map(|k| self.properties.get(*k).cloned())
    }

    /// `//[user[:password]@]host[:port][,host2…][/database][?key=value&…]`, or
    /// PostgreSQL's `database` alone. Of several hosts, the first one is kept: the drivers
    /// try them in order.
    fn hierarchical(driver: DriverKind, rest: &str) -> Result<Self> {
        let mut jdbc = Self::new(driver);
        let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            jdbc.properties
                .insert(key.to_ascii_lowercase(), decode(value)?);
        }
        match rest.strip_prefix("//") {
            Some(rest) => {
                let rest = match rest.rsplit_once('@') {
                    Some((userinfo, rest)) => {
                        jdbc.set_login(userinfo)?;
                        rest
                    }
                    None => rest,
                };
                let (address, database) = rest.split_once('/').unwrap_or((rest, ""));
                let mut hosts = address.split(',');
                jdbc.set_address(hosts.next().unwrap_or_default());
                jdbc.other_hosts = hosts.filter(|h| !h.is_empty()).map(str::to_owned).collect();
                jdbc.config.database = decode(database)?;
            }
            None => jdbc.config.database = decode(rest)?,
        }
        jdbc.common_properties();
        let ssl = match driver {
            DriverKind::MySql => jdbc.mysql_ssl(),
            _ => jdbc
                .property(&["sslmode"])
                .and_then(|m| m.parse().ok())
                // pgjdbc's `ssl=true` verifies the certificate and the host name.
                .or_else(|| {
                    (jdbc.property(&["ssl"]).as_deref() == Some("true"))
                        .then_some(SslMode::VerifyFull)
                }),
        };
        if let Some(mode) = ssl {
            jdbc.config.ssl_mode = mode;
        }
        Ok(jdbc)
    }

    /// `user[:password]` before the host. A `/` in it means the URL is not percent-encoded,
    /// so where the login ends is unclear: an error, which does not repeat it.
    fn set_login(&mut self, userinfo: &str) -> Result<()> {
        if userinfo.contains('/') {
            return Err(Error::Config(
                "the user and password of the JDBC URL must be percent-encoded".to_owned(),
            ));
        }
        let (user, password) = match userinfo.split_once(':') {
            Some((user, password)) => (user, Some(password)),
            None => (userinfo, None),
        };
        self.config.user = decode(user)?;
        if let Some(password) = password {
            self.config.password = Some(Secret::new(decode(password)?));
        }
        Ok(())
    }

    /// Connector/J's `sslMode` (`REQUIRED`, `VERIFY_IDENTITY`, …) or `useSSL`.
    fn mysql_ssl(&self) -> Option<SslMode> {
        let mode = self.property(&["sslmode"]).unwrap_or_default();
        match mode.to_ascii_lowercase().as_str() {
            "disabled" => Some(SslMode::Disable),
            "preferred" => Some(SslMode::Prefer),
            "required" => Some(SslMode::Require),
            "verify_ca" => Some(SslMode::VerifyCa),
            "verify_identity" => Some(SslMode::VerifyFull),
            _ => None,
        }
        .or_else(|| match self.property(&["usessl"]).as_deref() {
            Some("false") => Some(SslMode::Disable),
            Some("true") => Some(SslMode::Require),
            _ => None,
        })
    }

    /// User, password and application name, by the names the drivers give them.
    fn common_properties(&mut self) {
        if let Some(user) = self.property(&["user", "username"]) {
            self.config.user = user;
        }
        if let Some(password) = self.property(&["password"]) {
            self.config.password = Some(Secret::new(password));
        }
        if let Some(name) = self.property(&["applicationname"]) {
            self.config.application_name = Some(name);
        }
    }

    /// `host[\instance][:port][;key=value…]`, or jTDS's `host[:port][/database][;key=value…]`.
    /// The properties may name the server, port, instance and database instead.
    fn sqlserver(rest: &str, jtds: bool) -> Self {
        let mut jdbc = Self::new(DriverKind::SqlServer);
        let (address, props) = rest.split_once(';').unwrap_or((rest, ""));
        let address = strip_tcp(address);
        jdbc.properties = sqlserver_properties(props);
        let (address, database) = match address.split_once('/') {
            Some((a, d)) if jtds => (a, d),
            _ => (address, ""),
        };
        let address = if address.is_empty() {
            jdbc.property(&["servername", "server"]).unwrap_or_default()
        } else {
            address.to_owned()
        };
        let (host, port) = host_port(&address);
        let (host, instance) = Self::split_instance(&host);
        jdbc.set_address(&host);
        let port = port.or_else(|| jdbc.property(&["portnumber", "port"])?.parse().ok());
        if let Some(port) = port {
            jdbc.config.port = Some(port);
            jdbc.port_given = true;
        }
        jdbc.instance = instance.or_else(|| jdbc.property(&["instancename", "instance"]));
        jdbc.config.database = if database.is_empty() {
            jdbc.property(&["databasename", "database"])
                .unwrap_or_default()
        } else {
            database.to_owned()
        };
        jdbc.common_properties();
        let encrypt = jdbc
            .property(&["encrypt"])
            .and_then(|v| sqlserver_encrypt(&v));
        let trust = jdbc.property(&["trustservercertificate"]).is_some_and(|v| {
            matches!(
                v.to_ascii_lowercase().as_str(),
                "true" | "strict" | "mandatory"
            )
        });
        if let Some(mode) = sqlserver_ssl(encrypt, trust) {
            jdbc.config.ssl_mode = mode;
        }
        jdbc
    }

    /// `//host:port/service`, `host:port/service`, `host:port:SID`, a net service name, or a
    /// connect descriptor, after `@`; a user and password may come first (`user/pass@…`).
    /// A SID is kept as the database, where the service name goes: they are often the same.
    fn oracle(rest: &str) -> Self {
        let mut jdbc = Self::new(DriverKind::Oracle);
        // The last `@`: a password may hold one.
        let rest = match rest.rsplit_once('@') {
            Some((credentials, rest)) if !credentials.starts_with('(') => {
                let (user, password) = credentials.split_once('/').unwrap_or((credentials, ""));
                user.clone_into(&mut jdbc.config.user);
                if !password.is_empty() {
                    jdbc.config.password = Some(Secret::new(password));
                }
                rest
            }
            _ => rest,
        };
        let rest = rest.split_once('?').map_or(rest, |(r, _)| r);
        if rest.starts_with('(') || !rest.contains([':', '/']) {
            // Resolved by the client (tnsnames.ora), with no host of its own.
            jdbc.config.host.clear();
            jdbc.config.port = None;
            rest.clone_into(&mut jdbc.config.database);
            return jdbc;
        }
        let rest = rest.strip_prefix("//").unwrap_or(rest);
        let (address, service) = match rest.split_once('/') {
            Some((address, service)) => (address, service.split(':').next().unwrap_or_default()),
            // `host:port:SID`
            None => match rest.rsplit_once(':') {
                Some((address, sid)) if address.contains(':') => (address, sid),
                _ => (rest, ""),
            },
        };
        jdbc.set_address(address);
        service.clone_into(&mut jdbc.config.database);
        jdbc
    }
}

/// `[::1]:5432`, `host:5432` or just a host. A `:` not followed by a port stays in the
/// host, where [`ConnectionConfig::check_host`] refuses it.
pub(super) fn host_port(address: &str) -> (String, Option<u16>) {
    let (host, port) = match address.strip_prefix('[') {
        Some(inner) => match inner.split_once(']') {
            Some((h, rest)) => (h, rest.strip_prefix(':').and_then(|p| p.parse().ok())),
            None => (inner, None),
        },
        None => match address.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') => match p.parse() {
                Ok(port) => (h, Some(port)),
                Err(_) => (address, None),
            },
            _ => (address, None),
        },
    };
    (host.trim().to_owned(), port)
}

/// A SQL Server address without its `tcp:` protocol prefix, written in any case.
pub(super) fn strip_tcp(address: &str) -> &str {
    match address.get(..4) {
        Some(p) if p.eq_ignore_ascii_case("tcp:") => &address[4..],
        _ => address,
    }
}

/// SQL Server's `encrypt`: `true`, `false`, or `None` for a value it does not take.
pub(super) fn sqlserver_encrypt(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" | "strict" | "mandatory" => Some(true),
        "false" | "no" | "0" | "optional" => Some(false),
        _ => None,
    }
}

/// The TLS mode of SQL Server's `encrypt` and `trustServerCertificate`: `None` when
/// `encrypt` is not given, so the driver's default stays.
pub(super) fn sqlserver_ssl(encrypt: Option<bool>, trust: bool) -> Option<SslMode> {
    match (encrypt?, trust) {
        (false, _) => Some(SslMode::Disable),
        (true, true) => Some(SslMode::Require),
        (true, false) => Some(SslMode::VerifyFull),
    }
}

/// `key=value;key={value}` with the keys in lower case. A value in braces may hold `;`,
/// and `}}` inside it is one brace.
fn sqlserver_properties(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut rest = text;
    while !rest.is_empty() {
        let end = rest.find([';', '=']).unwrap_or(rest.len());
        if rest[end..].starts_with('=') {
            let key = rest[..end].trim().to_ascii_lowercase();
            let (value, after) = sqlserver_value(&rest[end + 1..]);
            if !key.is_empty() {
                out.insert(key, value);
            }
            rest = after;
        } else {
            // A part without `=`: skipped.
            rest = &rest[end..];
        }
        rest = rest.strip_prefix(';').unwrap_or(rest);
    }
    out
}

/// The value at the start of `text` and what follows it (from its `;`).
fn sqlserver_value(text: &str) -> (String, &str) {
    let trimmed = text.trim_start();
    if let Some(inner) = trimmed.strip_prefix('{') {
        let mut value = String::new();
        let mut chars = inner.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c != '}' {
                value.push(c);
            } else if chars.peek().is_some_and(|&(_, next)| next == '}') {
                value.push('}');
                chars.next();
            } else {
                let after = &inner[i + 1..];
                let end = after.find(';').unwrap_or(after.len());
                return (value, &after[end..]);
            }
        }
        return (value, "");
    }
    let end = text.find(';').unwrap_or(text.len());
    (text[..end].trim().to_owned(), &text[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(jdbc: &JdbcUrl) -> (DriverKind, &str, Option<u16>, &str) {
        let c = &jdbc.config;
        (c.driver, c.host.as_str(), c.port, c.database.as_str())
    }

    fn parse(url: &str) -> JdbcUrl {
        JdbcUrl::parse(url).unwrap()
    }

    #[test]
    fn sqlserver_urls() {
        use DriverKind::SqlServer as Ms;
        let plain = parse("jdbc:sqlserver://db01:1433;databaseName=Vericode;user=sa;password=pw");
        assert_eq!(at(&plain), (Ms, "db01", Some(1433), "Vericode"));
        assert!(plain.port_given);
        assert_eq!(plain.config.user, "sa");
        assert_eq!(
            plain.config.password.as_ref().map(Secret::expose),
            Some("pw")
        );

        let named = parse(r"jdbc:sqlserver://SRV\SQLEXPRESS;database=App");
        assert_eq!(at(&named), (Ms, "SRV", Some(1433), "App"));
        assert_eq!(named.instance.as_deref(), Some("SQLEXPRESS"));
        assert!(!named.port_given);

        let with_port = parse(r"jdbc:sqlserver://SRV\PROD:14330;databaseName=x");
        assert_eq!(at(&with_port), (Ms, "SRV", Some(14330), "x"));
        assert!(with_port.port_given);

        let by_props =
            parse("jdbc:sqlserver://;serverName=10.0.0.5;portNumber=1500;databaseName=x");
        assert_eq!(at(&by_props), (Ms, "10.0.0.5", Some(1500), "x"));
        let braces = parse("jdbc:sqlserver://host;instanceName=PROD;DatabaseName={My Db}");
        assert_eq!(at(&braces), (Ms, "host", Some(1433), "My Db"));
        assert_eq!(braces.instance.as_deref(), Some("PROD"));
        let jtds = parse("jdbc:jtds:sqlserver://legacy:1434/erp;instance=X;domain=CORP");
        assert_eq!(at(&jtds), (Ms, "legacy", Some(1434), "erp"));
        assert_eq!(
            jtds.properties.get("domain").map(String::as_str),
            Some("CORP")
        );
        let old = parse("jdbc:microsoft:sqlserver://old:1433;DatabaseName=crm");
        assert_eq!(at(&old), (Ms, "old", Some(1433), "crm"));
        assert_eq!(
            at(&parse("jdbc:sqlserver://[::1]:1433")),
            (Ms, "::1", Some(1433), "")
        );

        let in_braces = parse("jdbc:sqlserver://h;user=sa;password={a;b}}c};databaseName=d");
        assert_eq!(
            in_braces.config.password.as_ref().map(Secret::expose),
            Some("a;b}c")
        );
        assert_eq!(in_braces.config.database, "d");
        assert_eq!(
            at(&parse("jdbc:sqlserver://tcp:h:1433")),
            (Ms, "h", Some(1433), "")
        );

        let tls = |url| parse(url).config.ssl_mode;
        assert_eq!(
            tls("jdbc:sqlserver://h;encrypt=true;trustServerCertificate=true"),
            SslMode::Require
        );
        assert_eq!(tls("jdbc:sqlserver://h;encrypt=true"), SslMode::VerifyFull);
        assert_eq!(tls("jdbc:sqlserver://h;encrypt=false"), SslMode::Disable);
        assert_eq!(tls("jdbc:sqlserver://h"), SslMode::Prefer);
    }

    #[test]
    fn postgres_mysql_clickhouse_urls() {
        let pg = parse("jdbc:postgresql://db:6543/app?user=me&sslmode=require");
        assert_eq!(at(&pg), (DriverKind::Postgres, "db", Some(6543), "app"));
        assert_eq!(
            (pg.config.user.as_str(), pg.config.ssl_mode),
            ("me", SslMode::Require)
        );
        let short = parse("jdbc:postgresql:shop");
        assert_eq!(
            at(&short),
            (DriverKind::Postgres, "localhost", Some(5432), "shop")
        );
        assert_eq!(at(&parse("jdbc:postgresql://[::1]/postgres")).2, Some(5432));

        let my = parse("jdbc:mysql://localhost:3307/shop?useSSL=false");
        assert_eq!(
            at(&my),
            (DriverKind::MySql, "localhost", Some(3307), "shop")
        );
        assert_eq!(my.config.ssl_mode, SslMode::Disable);
        let maria = parse("jdbc:mariadb://m/db?sslMode=VERIFY_IDENTITY");
        assert_eq!(maria.config.ssl_mode, SslMode::VerifyFull);
        let ch = parse("jdbc:clickhouse://ch:8123/logs");
        assert_eq!(at(&ch), (DriverKind::ClickHouse, "ch", Some(8123), "logs"));
        assert_eq!(
            parse("jdbc:postgresql://h/db?ssl=true").config.ssl_mode,
            SslMode::VerifyFull
        );
        let many = parse("jdbc:postgresql://h1:5433,h2:5434/db");
        assert_eq!(at(&many), (DriverKind::Postgres, "h1", Some(5433), "db"));
    }

    #[test]
    fn logins_before_the_host_go_to_their_fields() {
        let my = parse("jdbc:mysql://sandy:hunter2@myhost:3307/db");
        assert_eq!(at(&my), (DriverKind::MySql, "myhost", Some(3307), "db"));
        assert_eq!(my.config.user, "sandy");
        assert_eq!(
            my.config.password.as_ref().map(Secret::expose),
            Some("hunter2")
        );
        let no_port = parse("jdbc:mariadb://sandy:p%40ss@myhost/db");
        assert_eq!(
            at(&no_port),
            (DriverKind::MySql, "myhost", Some(3306), "db")
        );
        assert_eq!(
            no_port.config.password.as_ref().map(Secret::expose),
            Some("p@ss")
        );
        let user_only = parse("jdbc:postgresql://me@h/db?sslmode=require");
        assert_eq!(
            (user_only.config.user.as_str(), user_only.config.password),
            ("me", None)
        );
        // Not percent-encoded, or not a host at all: an error that does not repeat it.
        for bad in [
            "jdbc:mysql://sandy:hun/ter2@myhost/db",
            "jdbc:mysql://sandy:hunter2/db",
            "jdbc:postgresql://h port=5432 password=hunter2/db",
        ] {
            let err = JdbcUrl::parse(bad).unwrap_err().to_string();
            assert!(!err.contains("hunter2"), "{err}");
        }
        let debug = format!("{:?}", parse("jdbc:sqlserver://h;password=hunter2;app=x"));
        assert!(
            !debug.contains("hunter2") && debug.contains("app"),
            "{debug}"
        );
    }

    #[test]
    fn oracle_urls() {
        use DriverKind::Oracle as Ora;
        assert_eq!(
            at(&parse("jdbc:oracle:thin:@db:1522/ORCLPDB")),
            (Ora, "db", Some(1522), "ORCLPDB")
        );
        assert_eq!(
            at(&parse("jdbc:oracle:thin:@//db/XEPDB1")),
            (Ora, "db", Some(1521), "XEPDB1")
        );
        assert_eq!(
            at(&parse("jdbc:oracle:thin:@db:1521:XE")),
            (Ora, "db", Some(1521), "XE")
        );
        let creds = parse("jdbc:oracle:thin:scott/ti@ger@db:1521/svc");
        assert_eq!(at(&creds), (Ora, "db", Some(1521), "svc"));
        assert_eq!(creds.config.user, "scott");
        assert_eq!(
            creds.config.password.as_ref().map(Secret::expose),
            Some("ti@ger")
        );
        assert_eq!(
            at(&parse("jdbc:oracle:thin:@PROD_TNS")),
            (Ora, "", None, "PROD_TNS")
        );
        let descriptor = "(DESCRIPTION=(ADDRESS=(PROTOCOL=TCP)(HOST=h)(PORT=1521))(CONNECT_DATA=(SERVICE_NAME=s)))";
        assert_eq!(
            at(&parse(&format!("jdbc:oracle:thin:@{descriptor}"))).3,
            descriptor
        );
    }

    #[test]
    fn embedded_and_unsupported_urls() {
        assert_eq!(
            at(&parse("jdbc:sqlite:C:/data/app.db")),
            (DriverKind::Sqlite, "", None, "C:/data/app.db")
        );
        assert_eq!(at(&parse("jdbc:duckdb:")).3, ":memory:");
        let err = JdbcUrl::parse("jdbc:db2://h:50000/x;password=secret").unwrap_err();
        assert!(
            err.to_string().contains("jdbc:db2:") && !err.to_string().contains("secret"),
            "{err}"
        );
        let err = JdbcUrl::parse("jdbc:foo;password=secret").unwrap_err();
        assert!(!err.to_string().contains("secret"), "{err}");
        assert!(JdbcUrl::parse("jdbc:jtds:sybase://s:5000/db").is_err());
        assert!(ConnectionConfig::from_jdbc_url("postgres://h/db").is_err());
        let config = ConnectionConfig::from_jdbc_url("jdbc:sqlserver://h;databaseName=d").unwrap();
        assert_eq!(
            (config.driver, config.database.as_str()),
            (DriverKind::SqlServer, "d")
        );
    }
}
