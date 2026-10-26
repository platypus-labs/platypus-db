//! Loading DuckDB: the app does not ship it, it downloads the shared library DuckDB publishes
//! (`libduckdb`) the first time a DuckDB database or a data file is opened, as the vendor
//! package [`PACKAGE`] of `platypus-drivers`. `platypus-duckdb-loader` loads it, once per
//! process.

use std::path::Path;
use std::sync::Mutex;

use platypus_core::{DriverKind, Error, Result};

/// The vendor package (`platypus_drivers::vendor_package`).
pub const PACKAGE: &str = "duckdb";

/// Whether the library is loaded; a failed load is tried again next time.
static LOADED: Mutex<bool> = Mutex::new(false);

/// Loads the library the app downloaded under its data folder. Without it, the driver is
/// missing and the app offers the download.
pub fn load() -> Result<()> {
    load_with(|| {
        platypus_drivers::data_dir()
            .and_then(|dir| platypus_drivers::installed_vendor(dir, PACKAGE))
            .map(|installed| installed.library)
            .ok_or(Error::DriverMissing {
                driver: DriverKind::DuckDb,
            })
    })
}

/// Loads the library at `path` unless one is loaded; [`load`] then finds it. For tests and
/// tools that keep the library elsewhere.
pub fn load_from(path: &Path) -> Result<()> {
    load_with(|| Ok(path.to_owned()))
}

fn load_with(find: impl FnOnce() -> Result<std::path::PathBuf>) -> Result<()> {
    let mut loaded = LOADED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !*loaded {
        let path = find()?;
        platypus_duckdb_loader::load(&path).map_err(|e| {
            Error::Connect(format!("cannot load DuckDB from {}: {e}", path.display()))
        })?;
        *loaded = true;
    }
    Ok(())
}
