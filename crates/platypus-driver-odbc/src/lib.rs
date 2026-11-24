//! Databases reached through an ODBC driver installed in the system: Db2 through IBM's Data
//! Server Driver, and any driver or data source the user picks. IBM's driver is installed by
//! the app (`platypus-drivers`); this crate finds it in the system's driver manager and
//! opens connections with it.
//!
//! ODBC is reached on Windows only for now: elsewhere the app would have to link unixODBC
//! and would not start without it.

use async_trait::async_trait;
use platypus_core::{Connection, ConnectionConfig, Driver, DriverKind, Result};

mod command;
mod connection;
mod jdbc;
pub mod meta;
pub mod profile;
#[cfg(windows)]
mod session;
pub mod system;

pub use connection::OdbcConnection;
pub use jdbc::discover::{FoundJars, find as find_jdbc_jars};
pub use jdbc::java_program;

/// Opens connections through the system's ODBC drivers.
#[derive(Debug, Clone, Copy, Default)]
pub struct OdbcDriver;

/// The connection string that opens `config`, with Db2's vendor driver (IBM i Access's for
/// Db2 for IBM i) found among the system's drivers unless the connection names one.
pub fn connection_string(config: &ConnectionConfig) -> Result<String> {
    let found = if config.options.contains_key(profile::DRIVER) {
        None
    } else if profile::is_ibm_i(config) {
        profile::ibm_i_driver(usable_drivers(&system::list()?))
    } else if config.driver.vendor_package().is_some() {
        vendor_driver_installed(config.driver)?
    } else {
        None
    };
    profile::connection_string(config, found.as_deref())
}

/// Whether the vendor driver `kind` needs is installed in the system.
pub fn vendor_driver_installed(kind: DriverKind) -> Result<Option<String>> {
    Ok(profile::vendor_driver(
        kind,
        usable_drivers(&system::list()?),
    ))
}

/// The drivers the driver manager can open (else `IM002`): not those left behind by a removed
/// client, whose library is gone (all are kept when no entry names its library), nor those
/// whose name is too long, like IBM's when registered from a deep folder.
fn usable_drivers(system: &system::SystemOdbc) -> Vec<&str> {
    let paths_known = system.drivers.iter().any(|d| d.path.is_some());
    system
        .drivers
        .iter()
        .filter(|d| d.name.len() <= profile::MAX_DRIVER_NAME)
        .filter(|d| {
            !paths_known
                || d.path
                    .as_deref()
                    .is_some_and(|p| std::path::Path::new(p).is_file())
        })
        .map(|d| d.name.as_str())
        .collect()
}

#[async_trait]
impl Driver for OdbcDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Odbc
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let conn: Box<dyn Connection> = Box::new(OdbcConnection::open(config).await?);
        platypus_core::run_init(&*conn, &config.init, config.driver).await?;
        Ok(conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drivers_left_behind_without_their_library_are_skipped() {
        let here = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let driver = |name: &str, path: Option<&str>| system::SystemDriver {
            name: name.to_owned(),
            path: path.map(str::to_owned),
        };
        let system = system::SystemOdbc {
            drivers: vec![
                driver(
                    "IBM DB2 ODBC DRIVER - DB2COPY1",
                    Some(r"C:\gone\db2cli64.dll"),
                ),
                driver("IBM DB2 ODBC DRIVER - IBMDBCL1", Some(&here)),
                driver("IBM DB2 ODBC DRIVER - OLD", None),
                driver(
                    "IBM DB2 ODBC DRIVER - C_Users_p.cislaghi_AppData_Roaming_dev.platypusdb.desktop_drivers_vendor_db2_12.1.4_clidriver",
                    Some(&here),
                ),
            ],
            data_sources: Vec::new(),
        };
        assert_eq!(usable_drivers(&system), ["IBM DB2 ODBC DRIVER - IBMDBCL1"]);
        let unknown = system::SystemOdbc {
            drivers: vec![driver("A", None), driver("B", None)],
            data_sources: Vec::new(),
        };
        assert_eq!(usable_drivers(&unknown), ["A", "B"]);
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
