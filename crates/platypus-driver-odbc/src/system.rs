//! The ODBC drivers and data sources (DSNs) the system's driver manager knows.

use serde::Serialize;

/// Whether this build reaches ODBC drivers: on Windows, through `odbc32.dll`.
pub const AVAILABLE: bool = cfg!(windows);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemDriver {
    pub name: String,
    /// The driver's library, as registered.
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DataSource {
    pub name: String,
    /// The name of its driver.
    pub driver: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SystemOdbc {
    pub drivers: Vec<SystemDriver>,
    pub data_sources: Vec<DataSource>,
}

impl SystemOdbc {
    pub fn driver_names(&self) -> impl Iterator<Item = &str> {
        self.drivers.iter().map(|d| d.name.as_str())
    }
}

/// The drivers and DSNs registered in the system, by name.
#[cfg(windows)]
pub fn list() -> platypus_core::Result<SystemOdbc> {
    use arrow_odbc::odbc_api;
    use platypus_core::Error;

    let failed = |e: odbc_api::Error| Error::Other(format!("cannot list the ODBC drivers: {e}"));
    let environment = odbc_api::environment().map_err(failed)?;
    let mut drivers: Vec<SystemDriver> = environment
        .drivers()
        .map_err(failed)?
        .into_iter()
        .map(|d| SystemDriver {
            path: d
                .attributes
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("Driver"))
                .map(|(_, v)| v.clone()),
            name: d.description,
        })
        .collect();
    drivers.sort_by(|a, b| a.name.cmp(&b.name));
    let mut data_sources: Vec<DataSource> = environment
        .data_sources()
        .map_err(failed)?
        .into_iter()
        .map(|d| DataSource {
            name: d.server_name,
            driver: d.driver,
        })
        .collect();
    data_sources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(SystemOdbc {
        drivers,
        data_sources,
    })
}

/// Nothing elsewhere: ODBC is reached on Windows only.
#[cfg(not(windows))]
pub fn list() -> platypus_core::Result<SystemOdbc> {
    Ok(SystemOdbc::default())
}
