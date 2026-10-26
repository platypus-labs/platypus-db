//! JDBC URLs, as DBeaver and DataGrip keep them: PostgreSQL, MySQL/MariaDB, SQLite and
//! SQL Server (Microsoft's driver, jTDS and the old `jdbc:microsoft:` one).

use std::collections::BTreeMap;

use platypus_core::DriverKind;

use crate::connections::SavedConnection;

/// What a JDBC URL says about the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Jdbc {
    pub driver: DriverKind,
    pub host: String,
    pub port: Option<u16>,
    pub database: String,
    /// A SQL Server named instance (`host\SQLEXPRESS` or `instanceName=`).
    pub instance: Option<String>,
    /// SQL Server's `;key=value` properties, keys in lower case.
    pub props: BTreeMap<String, String>,
}

impl Jdbc {
    fn new(driver: DriverKind) -> Self {
        Self {
            driver,
            host: String::new(),
            port: None,
            database: String::new(),
            instance: None,
            props: BTreeMap::new(),
        }
    }

    /// Fills driver, host, port (the driver's default without one) and database.
    pub fn apply(&self, c: &mut SavedConnection) {
        c.driver = self.driver;
        self.host.clone_into(&mut c.host);
        c.port = self.port.or(self.driver.default_port());
        self.database.clone_into(&mut c.database);
    }
}

/// The server of a JDBC URL; `None` for other databases or another syntax.
pub(super) fn parse(url: &str) -> Option<Jdbc> {
    let rest = url.trim().strip_prefix("jdbc:")?;
    if let Some(path) = rest.strip_prefix("sqlite:") {
        let mut jdbc = Jdbc::new(DriverKind::Sqlite);
        path.clone_into(&mut jdbc.database);
        return Some(jdbc);
    }
    for prefix in [
        "sqlserver://",
        "jtds:sqlserver://",
        "microsoft:sqlserver://",
    ] {
        if let Some(r) = rest.strip_prefix(prefix) {
            return Some(sqlserver(r, prefix.starts_with("jtds")));
        }
    }
    let (driver, rest) = [
        ("postgresql://", DriverKind::Postgres),
        ("mysql://", DriverKind::MySql),
        ("mariadb://", DriverKind::MySql),
    ]
    .into_iter()
    .find_map(|(prefix, driver)| Some((driver, rest.strip_prefix(prefix)?)))?;
    let mut jdbc = Jdbc::new(driver);
    let rest = rest.split(['?', ';']).next().unwrap_or_default();
    let (address, database) = rest.split_once('/').unwrap_or((rest, ""));
    (jdbc.host, jdbc.port) = host_port(address);
    database.clone_into(&mut jdbc.database);
    Some(jdbc)
}

/// Fills driver, host, port and database from a JDBC URL; `false` for other databases.
pub(super) fn apply(c: &mut SavedConnection, url: &str) -> bool {
    parse(url).map(|jdbc| jdbc.apply(c)).is_some()
}

/// `[::1]:5432`, `host:5432` or just a host.
fn host_port(address: &str) -> (String, Option<u16>) {
    let (host, port) = match address.strip_prefix('[') {
        Some(inner) => match inner.split_once(']') {
            Some((h, rest)) => (h, rest.strip_prefix(':').and_then(|p| p.parse().ok())),
            None => (inner, None),
        },
        None => match address.rsplit_once(':') {
            Some((h, p)) => (h, p.parse().ok()),
            None => (address, None),
        },
    };
    (host.trim().to_owned(), port)
}

/// `host[\instance][:port][;key=value…]`, or jTDS's `host[:port][/database][;key=value…]`.
/// The properties may name the server, port, instance and database instead.
fn sqlserver(rest: &str, jtds: bool) -> Jdbc {
    let mut jdbc = Jdbc::new(DriverKind::SqlServer);
    let (address, props) = rest.split_once(';').unwrap_or((rest, ""));
    jdbc.props = properties(props);
    let (address, database) = match address.split_once('/') {
        Some((a, d)) if jtds => (a, d),
        _ => (address, ""),
    };
    let prop = |keys: &[&str]| keys.iter().find_map(|k| jdbc.props.get(*k).cloned());
    let (host, port) = host_port(address);
    let host = if host.is_empty() {
        prop(&["servername", "server"]).unwrap_or_default()
    } else {
        host
    };
    let (host, instance) = split_instance(&host);
    jdbc.host = host;
    jdbc.port = port.or_else(|| prop(&["portnumber", "port"]).and_then(|p| p.parse().ok()));
    jdbc.instance = instance.or_else(|| prop(&["instancename", "instance"]));
    jdbc.database = if database.is_empty() {
        prop(&["databasename", "database"]).unwrap_or_default()
    } else {
        database.to_owned()
    };
    jdbc
}

/// `srv\SQLEXPRESS` as the host and the instance.
pub(super) fn split_instance(host: &str) -> (String, Option<String>) {
    match host.split_once('\\') {
        Some((h, i)) if !i.trim().is_empty() => (h.trim().to_owned(), Some(i.trim().to_owned())),
        Some((h, _)) => (h.trim().to_owned(), None),
        None => (host.to_owned(), None),
    }
}

/// `key=value;key={value}` with the keys in lower case.
fn properties(text: &str) -> BTreeMap<String, String> {
    text.split(';')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| {
            let v = v.trim();
            let v = v
                .strip_prefix('{')
                .and_then(|v| v.strip_suffix('}'))
                .unwrap_or(v);
            (k.trim().to_ascii_lowercase(), v.to_owned())
        })
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(url: &str) -> (String, Option<u16>, String, Option<String>) {
        let j = parse(url).unwrap();
        assert_eq!(j.driver, DriverKind::SqlServer, "{url}");
        (j.host, j.port, j.database, j.instance)
    }

    #[test]
    fn sqlserver_urls() {
        let own = |s: &str| s.to_owned();
        assert_eq!(
            server("jdbc:sqlserver://db01:1433;databaseName=Vericode;encrypt=true"),
            (own("db01"), Some(1433), own("Vericode"), None)
        );
        assert_eq!(
            server(r"jdbc:sqlserver://SRV\SQLEXPRESS;database=App"),
            (own("SRV"), None, own("App"), Some(own("SQLEXPRESS")))
        );
        assert_eq!(
            server("jdbc:sqlserver://;serverName=10.0.0.5;portNumber=1500;databaseName=x"),
            (own("10.0.0.5"), Some(1500), own("x"), None)
        );
        assert_eq!(
            server("jdbc:sqlserver://host;instanceName=PROD;DatabaseName={My Db}"),
            (own("host"), None, own("My Db"), Some(own("PROD")))
        );
        assert_eq!(
            server("jdbc:jtds:sqlserver://legacy:1434/erp;instance=X;domain=CORP"),
            (own("legacy"), Some(1434), own("erp"), Some(own("X")))
        );
        assert_eq!(
            server("jdbc:microsoft:sqlserver://old:1433;DatabaseName=crm"),
            (own("old"), Some(1433), own("crm"), None)
        );
        assert_eq!(
            server("jdbc:sqlserver://[::1]:1433"),
            (own("::1"), Some(1433), own(""), None)
        );
        let azure = parse(
            "jdbc:sqlserver://acme.database.windows.net:1433;database=sales;authentication=ActiveDirectoryPassword",
        )
        .unwrap();
        assert_eq!(azure.host, "acme.database.windows.net");
        assert_eq!(
            azure.props.get("authentication").map(String::as_str),
            Some("ActiveDirectoryPassword")
        );
    }

    #[test]
    fn other_urls() {
        let pg = parse("jdbc:postgresql://db:6543/app?ssl=true").unwrap();
        assert_eq!(
            (pg.driver, pg.host.as_str(), pg.port, pg.database.as_str()),
            (DriverKind::Postgres, "db", Some(6543), "app")
        );
        assert!(parse("jdbc:oracle:thin:@x").is_none());
        assert!(parse("jdbc:jtds:sybase://x:5000/db").is_none());
        let mut c = SavedConnection::new("c", DriverKind::Postgres);
        assert!(apply(&mut c, "jdbc:sqlserver://h"));
        assert_eq!((c.driver, c.port), (DriverKind::SqlServer, Some(1433)));
    }
}
