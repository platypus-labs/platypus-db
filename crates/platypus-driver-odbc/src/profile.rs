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
/// The `options` key of the platform a Db2 server runs on: `zos`, `ibmi`, or Db2 for Linux,
/// Unix and Windows (LUW) when absent.
pub const DB2_PLATFORM: &str = "db2_platform";

/// Whether `config` is Db2 for IBM i, which IBM i Access's own ODBC driver reaches.
#[must_use]
pub fn is_ibm_i(config: &ConnectionConfig) -> bool {
    config.driver == DriverKind::Db2 && option(config, DB2_PLATFORM) == Some("ibmi")
}

pub(crate) fn option<'a>(config: &'a ConnectionConfig, key: &str) -> Option<&'a str> {
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

/// Appends the user and password, when given and not in `out` already, as `UID` and `PWD`.
fn push_login(out: &mut String, config: &ConnectionConfig) {
    if !config.user.is_empty() && !has_key(out, "UID") {
        push(out, "UID", &config.user);
    }
    if let Some(password) = config.password.as_ref().filter(|p| !p.expose().is_empty())
        && !has_key(out, "PWD")
    {
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
pub fn vendor_driver<'a>(
    kind: DriverKind,
    installed: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    if kind != DriverKind::Db2 {
        return None;
    }
    let matches = |name: &str| {
        let name = name.to_ascii_uppercase();
        name.starts_with("IBM DB2 ODBC DRIVER") || name.starts_with("IBM DATA SERVER")
    };
    installed
        .into_iter()
        .filter(|n| matches(n))
        .max()
        .map(str::to_owned)
}

/// IBM i Access's ODBC driver among the system's (`installed`): its current name first, then
/// the older iSeries Access and Client Access ones.
#[must_use]
pub fn ibm_i_driver<'a>(installed: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let installed: Vec<&str> = installed.into_iter().collect();
    [
        "IBM I ACCESS ODBC",
        "ISERIES ACCESS ODBC",
        "CLIENT ACCESS ODBC",
    ]
    .iter()
    .find_map(|p| {
        installed
            .iter()
            .find(|n| n.to_ascii_uppercase().starts_with(p))
    })
    .map(|n| (*n).to_owned())
}

/// The connection string that opens `config`. `driver` is the ODBC driver to name for Db2,
/// found in the system unless the connection names one.
pub fn connection_string(config: &ConnectionConfig, driver: Option<&str>) -> Result<String> {
    let mut out = String::new();
    match config.driver {
        DriverKind::Db2 if is_ibm_i(config) => {
            let driver = option(config, DRIVER).or(driver).ok_or_else(|| {
                Error::Config(
                    "Db2 for IBM i needs IBM's \"IBM i Access ODBC Driver\" (IBM i Access Client \
                     Solutions): install it, or name the driver in the connection's options"
                        .to_owned(),
                )
            })?;
            push(&mut out, "DRIVER", driver);
            ibm_i(&mut out, config)?;
            push_login(&mut out, config);
        }
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
    // Through an SSH tunnel: its local end.
    if let Some(addr) = config.connect_addr {
        push(out, "HOSTNAME", &addr.ip().to_string());
        push(out, "PORT", &addr.port().to_string());
    } else {
        // Db2 for z/OS listens on DRDA's port; `DATABASE` is then its location name.
        let port = if option(config, DB2_PLATFORM) == Some("zos") {
            446
        } else {
            50000
        };
        push(out, "HOSTNAME", host);
        push(out, "PORT", &config.port.unwrap_or(port).to_string());
    }
    push(out, "PROTOCOL", "TCPIP");
    if !matches!(config.ssl_mode, SslMode::Disable | SslMode::Prefer) {
        push(out, "Security", "SSL");
        if let Some(ca) = &config.ssl_root_cert {
            push(out, "SSLServerCertificate", &ca.to_string_lossy());
        }
    }
    Ok(())
}

/// Db2 for IBM i: the system's name, the database as the default library, TLS unless disabled
/// or merely preferred. The driver opens several of the server's ports, which one SSH tunnel
/// does not carry.
fn ibm_i(out: &mut String, config: &ConnectionConfig) -> Result<()> {
    if config.connect_addr.is_some() {
        return Err(Error::Config(
            "Db2 for IBM i cannot go through an SSH tunnel: its driver opens several of the \
             server's ports"
                .to_owned(),
        ));
    }
    let host = config.host.trim();
    if host.is_empty() {
        return Err(Error::Config(
            "the IBM i system name is required".to_owned(),
        ));
    }
    push(out, "SYSTEM", host);
    if !config.database.trim().is_empty() {
        push(out, "DBQ", config.database.trim());
    }
    if !matches!(config.ssl_mode, SslMode::Disable | SslMode::Prefer) {
        push(out, "SSL", "1");
    }
    Ok(())
}

/// The value of `key` in the connection string `s`, without its braces.
fn key_value(s: &str, key: &str) -> Option<String> {
    s.split(';').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        let v = v.trim();
        let v = v
            .strip_prefix('{')
            .and_then(|v| v.strip_suffix('}'))
            .map_or_else(|| v.to_owned(), |v| v.replace("}}", "}"));
        (k.trim().eq_ignore_ascii_case(key) && !v.is_empty()).then_some(v)
    })
}

/// The longest driver name taken as one Windows' driver manager opens: it lists longer ones,
/// but connecting through them fails with `IM002` (65 characters opened, 80 did not).
pub const MAX_DRIVER_NAME: usize = 64;

/// Why the driver manager found neither the driver nor the data source the connection string
/// `s` names (ODBC's `IM002`), given the drivers and DSNs the system has.
#[must_use]
pub fn not_found_hint(s: &str, drivers: &[&str], dsns: &[&str]) -> String {
    let listed = |names: &[&str]| {
        if names.is_empty() {
            "none".to_owned()
        } else {
            names.join(", ")
        }
    };
    let bits = "PlatypusDB is 64-bit: it sees only the drivers and data sources of the 64-bit \
                ODBC Data Sources tool, not those set up in the 32-bit one";
    if let Some(driver) = key_value(s, "DRIVER") {
        if driver.len() > MAX_DRIVER_NAME && drivers.contains(&driver.as_str()) {
            return format!(
                "the ODBC driver \"{driver}\" is installed, but its name is too long for \
                 Windows to open it ({} characters; up to {MAX_DRIVER_NAME} are safe): \
                 register it under a shorter name",
                driver.len()
            );
        }
        format!(
            "no ODBC driver is named \"{driver}\". Installed drivers: {}. {bits}",
            listed(drivers)
        )
    } else if let Some(dsn) = key_value(s, "DSN") {
        format!(
            "no data source is named \"{dsn}\". Data sources: {}. {bits}",
            listed(dsns)
        )
    } else {
        "the connection string names no DRIVER or DSN: choose one, or write DRIVER={…} in it"
            .to_owned()
    }
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
    push_login(out, config);
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
        c.connect_addr = Some("127.0.0.1:40002".parse().unwrap());
        let s = connection_string(&c, Some("x")).unwrap();
        assert!(s.contains("HOSTNAME=127.0.0.1;PORT=40002;"), "{s}");
        c.database.clear();
        assert!(connection_string(&c, Some("x")).is_err());
    }

    #[test]
    fn not_found_names_what_is_missing_and_what_is_there() {
        let drivers = ["IBM DB2 ODBC DRIVER", "SQL Server"];
        let hint = not_found_hint("DRIVER={DB2 for z/OS};HOSTNAME=h;", &drivers, &[]);
        assert!(
            hint.starts_with("no ODBC driver is named \"DB2 for z/OS\""),
            "{hint}"
        );
        assert!(hint.contains("IBM DB2 ODBC DRIVER, SQL Server"), "{hint}");
        let hint = not_found_hint("dsn=AS400;UID=u;", &drivers, &[]);
        assert!(
            hint.starts_with("no data source is named \"AS400\". Data sources: none."),
            "{hint}"
        );
        assert!(not_found_hint("Server=h;", &drivers, &[]).contains("no DRIVER or DSN"));
        let long = format!("IBM DB2 ODBC DRIVER - {}", "x".repeat(60));
        let hint = not_found_hint(&format!("DRIVER={long};"), &[&long], &[]);
        assert!(hint.contains("too long for Windows"), "{hint}");
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
        // The password goes with a user written in the connection string.
        c.options.insert(
            CONNECTION_STRING.to_owned(),
            "Driver={x};Server=h;UID=sa".to_owned(),
        );
        assert_eq!(
            connection_string(&c, None).unwrap(),
            "Driver={x};Server=h;UID=sa;PWD=pw;"
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
        ];
        assert_eq!(
            vendor_driver(DriverKind::Db2, installed).as_deref(),
            Some("IBM DB2 ODBC DRIVER - IBMDBCL1")
        );
        // Oracle has its own native driver.
        assert_eq!(vendor_driver(DriverKind::Oracle, installed), None);
        assert_eq!(vendor_driver(DriverKind::Odbc, installed), None);
        assert_eq!(
            vendor_driver(DriverKind::Db2, installed[..2].iter().copied()),
            None
        );
    }

    #[test]
    fn db2_for_zos_defaults_to_the_drda_port() {
        let mut c = config(DriverKind::Db2);
        c.host = "mainframe".to_owned();
        c.database = "DSNLOC1".to_owned();
        c.ssl_mode = SslMode::Disable;
        c.port = None;
        c.options.insert(DB2_PLATFORM.to_owned(), "zos".to_owned());
        let s = connection_string(&c, Some("IBM DB2 ODBC DRIVER")).unwrap();
        assert!(
            s.contains("DATABASE=DSNLOC1;HOSTNAME=mainframe;PORT=446;"),
            "{s}"
        );
        c.port = Some(5021);
        let s = connection_string(&c, Some("IBM DB2 ODBC DRIVER")).unwrap();
        assert!(s.contains("PORT=5021;"), "{s}");
        assert!(!is_ibm_i(&c));
    }

    #[test]
    fn db2_for_ibm_i_uses_ibm_i_access() {
        let mut c = config(DriverKind::Db2);
        c.password = Some(Secret::new("pw"));
        c.host = "as400.local".to_owned();
        c.ssl_mode = SslMode::Prefer;
        c.options.insert(DB2_PLATFORM.to_owned(), "ibmi".to_owned());
        assert!(is_ibm_i(&c));
        // The IBM CLI driver found for Db2 LUW does not open IBM i.
        assert!(connection_string(&c, None).is_err());
        assert_eq!(
            connection_string(&c, Some("IBM i Access ODBC Driver")).unwrap(),
            "DRIVER=IBM i Access ODBC Driver;SYSTEM=as400.local;UID=scott;PWD=pw;"
        );
        c.database = "MYLIB".to_owned();
        c.ssl_mode = SslMode::Require;
        let s = connection_string(&c, Some("IBM i Access ODBC Driver")).unwrap();
        assert!(s.contains("SYSTEM=as400.local;DBQ=MYLIB;SSL=1;"), "{s}");
        // A driver named in the options wins.
        c.options
            .insert(DRIVER.to_owned(), "iSeries Access ODBC Driver".to_owned());
        let s = connection_string(&c, Some("IBM i Access ODBC Driver")).unwrap();
        assert!(s.starts_with("DRIVER=iSeries Access ODBC Driver;"), "{s}");
        c.connect_addr = Some("127.0.0.1:40002".parse().unwrap());
        assert!(connection_string(&c, None).is_err(), "no SSH tunnel");
        c.connect_addr = None;
        c.host.clear();
        assert!(connection_string(&c, None).is_err());
        // The platform is Db2's only.
        c.driver = DriverKind::Odbc;
        assert!(!is_ibm_i(&c));
    }

    #[test]
    fn finds_ibm_i_access_drivers() {
        let installed = [
            "IBM DB2 ODBC DRIVER - IBMDBCL1",
            "iSeries Access ODBC Driver",
            "IBM i Access ODBC Driver",
        ];
        assert_eq!(
            ibm_i_driver(installed).as_deref(),
            Some("IBM i Access ODBC Driver")
        );
        assert_eq!(
            ibm_i_driver(["Client Access ODBC Driver (32-bit)"]).as_deref(),
            Some("Client Access ODBC Driver (32-bit)")
        );
        assert_eq!(ibm_i_driver(installed[..1].iter().copied()), None);
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
