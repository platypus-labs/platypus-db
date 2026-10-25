//! The connection string each database's ODBC driver expects, from the connection's fields.
//! Keys checked against the documentation of IBM's Data Server Driver for ODBC and CLI.

use platypus_core::{ConnectionConfig, DriverKind, Error, Result, SslMode};

/// The `options` key naming the ODBC driver: the driver of the generic connection, or one
/// that replaces the vendor driver found in the system.
pub const DRIVER: &str = "driver";
/// The `options` key of a data source name (DSN) the system knows.
pub const DSN: &str = "dsn";
/// The `options` key of a connection string written by the user, used as it is.
pub const CONNECTION_STRING: &str = "connection_string";

fn option<'a>(config: &'a ConnectionConfig, key: &str) -> Option<&'a str> {
    config
        .options
        .get(key)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
}

/// A connection string value, in braces when it holds what would end or split it.
fn value(v: &str) -> String {
    if v.contains([';', '{', '}', '=']) || v.trim() != v {
        format!("{{{}}}", v.replace('}', "}}"))
    } else {
        v.to_owned()
    }
}

/// Appends `key=value;`.
fn push(out: &mut String, key: &str, v: &str) {
    out.push_str(key);
    out.push('=');
    out.push_str(&value(v));
    out.push(';');
}

/// Appends the user and password, when given, as `UID` and `PWD`.
fn push_login(out: &mut String, config: &ConnectionConfig) {
    if !config.user.is_empty() {
        push(out, "UID", &config.user);
    }
    if let Some(password) = config.password.as_ref().filter(|p| !p.expose().is_empty()) {
        push(out, "PWD", password.expose());
    }
}

/// Whether the connection string `s` sets `key` already.
fn has_key(s: &str, key: &str) -> bool {
    s.split(';').any(|pair| {
        pair.split_once('=')
            .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key))
    })
}

/// The vendor driver of `kind` among the drivers the system has (`installed`): IBM's Db2
/// driver, under the name `db2cli install -setup` gives it or the one of a full client.
/// `None` for the generic driver.
#[must_use]
pub fn vendor_driver(kind: DriverKind, installed: &[String]) -> Option<String> {
    let matches = |name: &str| {
        let name = name.to_ascii_uppercase();
        kind == DriverKind::Db2
            && (name.starts_with("IBM DB2 ODBC DRIVER") || name.starts_with("IBM DATA SERVER"))
    };
    installed.iter().filter(|n| matches(n)).max().cloned()
}

/// The connection string that opens `config`. `driver` is the ODBC driver to name for Db2,
/// found in the system unless the connection names one.
pub fn connection_string(config: &ConnectionConfig, driver: Option<&str>) -> Result<String> {
    let mut out = String::new();
    match config.driver {
        DriverKind::Db2 => {
            let driver = option(config, DRIVER)
                .or(driver)
                .ok_or(Error::DriverMissing {
                    driver: config.driver,
                })?;
            push(&mut out, "DRIVER", driver);
            db2(&mut out, config)?;
            push_login(&mut out, config);
        }
        DriverKind::Odbc => generic(&mut out, config)?,
        other => {
            return Err(Error::Config(format!(
                "{} does not use an ODBC driver",
                other.name()
            )));
        }
    }
    Ok(out)
}

/// TCP/IP to `host:port`, TLS unless disabled or merely preferred (Db2 servers listen for it
/// on another port).
fn db2(out: &mut String, config: &ConnectionConfig) -> Result<()> {
    let host = config.host.trim();
    if host.is_empty() || config.database.trim().is_empty() {
        return Err(Error::Config(
            "the Db2 host and database are required".to_owned(),
        ));
    }
    push(out, "DATABASE", config.database.trim());
    push(out, "HOSTNAME", host);
    push(out, "PORT", &config.port.unwrap_or(50000).to_string());
    push(out, "PROTOCOL", "TCPIP");
    if !matches!(config.ssl_mode, SslMode::Disable | SslMode::Prefer) {
        push(out, "Security", "SSL");
        if let Some(ca) = &config.ssl_root_cert {
            push(out, "SSLServerCertificate", &ca.to_string_lossy());
        }
    }
    Ok(())
}

/// A connection string written by the user (after `DRIVER` when a driver is chosen), or a
/// DSN; the user and password are added unless already there.
fn generic(out: &mut String, config: &ConnectionConfig) -> Result<()> {
    let written = option(config, CONNECTION_STRING);
    if let Some(driver) =
        option(config, DRIVER).filter(|_| !written.is_some_and(|w| has_key(w, "DRIVER")))
    {
        push(out, "DRIVER", driver);
    }
    if let Some(written) = written {
        out.push_str(written);
        if !written.ends_with(';') {
            out.push(';');
        }
    } else if let Some(dsn) = option(config, DSN) {
        push(out, "DSN", dsn);
    } else if out.is_empty() {
        return Err(Error::Config(
            "choose a data source (DSN) or write a connection string".to_owned(),
        ));
    }
    let written = out.clone();
    if !has_key(&written, "UID") && !has_key(&written, "PWD") {
        push_login(out, config);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use platypus_core::Secret;

    use super::*;

    fn config(driver: DriverKind) -> ConnectionConfig {
        let mut c = ConnectionConfig::new(driver);
        c.user = "scott".to_owned();
        c.password = Some(Secret::new("ti;ger}"));
        c
    }

    #[test]
    fn db2_connection_string() {
        let mut c = config(DriverKind::Db2);
        c.password = Some(Secret::new("pw"));
        c.host = "db2.local".to_owned();
        c.database = "SAMPLE".to_owned();
        c.ssl_mode = SslMode::Prefer;
        assert_eq!(
            connection_string(&c, Some("IBM DB2 ODBC DRIVER - IBMDBCL1")).unwrap(),
            "DRIVER=IBM DB2 ODBC DRIVER - IBMDBCL1;DATABASE=SAMPLE;HOSTNAME=db2.local;PORT=50000;\
             PROTOCOL=TCPIP;UID=scott;PWD=pw;"
        );
        c.ssl_mode = SslMode::VerifyFull;
        c.port = Some(50001);
        let s = connection_string(&c, Some("IBM DB2 ODBC DRIVER")).unwrap();
        assert!(s.contains("PORT=50001;PROTOCOL=TCPIP;Security=SSL;"), "{s}");
        c.database.clear();
        assert!(connection_string(&c, Some("x")).is_err());
    }

    #[test]
    fn generic_connection_string() {
        let mut c = config(DriverKind::Odbc);
        c.password = Some(Secret::new("pw"));
        assert!(connection_string(&c, None).is_err());
        c.options.insert(DSN.to_owned(), "Sales DB".to_owned());
        assert_eq!(
            connection_string(&c, None).unwrap(),
            "DSN=Sales DB;UID=scott;PWD=pw;"
        );
        // A written connection string wins over the DSN and keeps its own login.
        c.options.insert(
            CONNECTION_STRING.to_owned(),
            "Server=localhost,51433;Database=platypus;UID=sa;PWD=x".to_owned(),
        );
        c.options.insert(DRIVER.to_owned(), "SQL Server".to_owned());
        assert_eq!(
            connection_string(&c, None).unwrap(),
            "DRIVER=SQL Server;Server=localhost,51433;Database=platypus;UID=sa;PWD=x;"
        );
        c.options.insert(
            CONNECTION_STRING.to_owned(),
            "driver={PostgreSQL Unicode};Server=h".to_owned(),
        );
        assert_eq!(
            connection_string(&c, None).unwrap(),
            "driver={PostgreSQL Unicode};Server=h;UID=scott;PWD=pw;"
        );
        assert!(connection_string(&config(DriverKind::Postgres), None).is_err());
    }

    #[test]
    fn finds_vendor_drivers() {
        let installed = [
            "SQL Server",
            "Oracle in instantclient_23_26",
            "IBM DB2 ODBC DRIVER - IBMDBCL1",
            "Microsoft ODBC for Oracle",
        ]
        .map(str::to_owned);
        assert_eq!(
            vendor_driver(DriverKind::Db2, &installed).as_deref(),
            Some("IBM DB2 ODBC DRIVER - IBMDBCL1")
        );
        // Oracle has its own native driver.
        assert_eq!(vendor_driver(DriverKind::Oracle, &installed), None);
        assert_eq!(vendor_driver(DriverKind::Odbc, &installed), None);
        assert_eq!(vendor_driver(DriverKind::Db2, &installed[..2]), None);
    }

    #[test]
    fn quotes_values() {
        assert_eq!(value("plain"), "plain");
        assert_eq!(value("a;b"), "{a;b}");
        assert_eq!(value("a}b"), "{a}}b}");
        assert_eq!(value(" x"), "{ x}");
        assert!(has_key("Driver={x};uid = a", "UID"));
        assert!(!has_key("Server=uid", "UID"));
    }
}
