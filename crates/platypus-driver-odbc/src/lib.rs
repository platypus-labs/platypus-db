//! Databases reached through an ODBC driver installed in the system: Oracle through Oracle's
//! Instant Client, Db2 through IBM's Data Server Driver, and any driver or data source the
//! user picks. The vendor drivers are installed by the app (`platypus-drivers`); this crate
//! finds them in the system's driver manager and opens connections with them.
//!
//! ODBC is reached on Windows only for now: elsewhere the app would have to link unixODBC
//! and would not start without it.

use async_trait::async_trait;
use platypus_core::{Connection, ConnectionConfig, Driver, DriverKind, Result};

#[cfg(windows)]
mod connection;
mod control;
pub mod meta;
pub mod profile;
#[cfg(windows)]
mod session;
pub mod system;

#[cfg(windows)]
pub use connection::OdbcConnection;

/// Opens connections through the system's ODBC drivers.
#[derive(Debug, Clone, Copy, Default)]
pub struct OdbcDriver;

/// The connection string that opens `config`, with the vendor driver of Oracle and Db2
/// found among the system's drivers unless the connection names one.
pub fn connection_string(config: &ConnectionConfig) -> Result<String> {
    let found = if config.driver.odbc_package().is_some()
        && !config.options.contains_key(profile::DRIVER)
    {
        profile::vendor_driver(config.driver, &system::list()?.driver_names())
    } else {
        None
    };
    profile::connection_string(config, found.as_deref())
}

/// Whether the vendor driver `kind` needs is installed in the system.
pub fn vendor_driver_installed(kind: DriverKind) -> Result<Option<String>> {
    Ok(profile::vendor_driver(
        kind,
        &system::list()?.driver_names(),
    ))
}

/// The statement as the database takes it: Oracle refuses a final `;` after plain SQL,
/// which PL/SQL needs.
#[must_use]
pub fn statement_text(kind: DriverKind, sql: &str) -> &str {
    if kind == DriverKind::Oracle && !platypus_sql::is_plsql(sql) {
        sql.trim_end().trim_end_matches(';').trim_end()
    } else {
        sql
    }
}

#[async_trait]
impl Driver for OdbcDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Odbc
    }

    #[cfg(windows)]
    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let conn: Box<dyn Connection> = Box::new(OdbcConnection::open(config).await?);
        platypus_core::run_init(&*conn, &config.init, config.driver).await?;
        Ok(conn)
    }

    #[cfg(not(windows))]
    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        Err(platypus_core::Error::Unsupported(format!(
            "{} through ODBC on this system: ODBC drivers are reached on Windows only for now",
            config.driver.name()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_statements_lose_their_final_semicolon() {
        assert_eq!(
            statement_text(DriverKind::Oracle, "select 1 from dual; \n"),
            "select 1 from dual"
        );
        assert_eq!(
            statement_text(DriverKind::Oracle, "BEGIN NULL; END;"),
            "BEGIN NULL; END;"
        );
        assert_eq!(statement_text(DriverKind::Db2, "values 1;"), "values 1;");
    }

    #[test]
    fn generic_connection_string_needs_no_system_lookup() {
        let mut config = ConnectionConfig::new(DriverKind::Odbc);
        config
            .options
            .insert(profile::DSN.to_owned(), "Sales".to_owned());
        assert_eq!(connection_string(&config).unwrap(), "DSN=Sales;");
    }
}
