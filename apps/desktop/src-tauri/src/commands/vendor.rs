//! The vendors' client libraries the app installs (Oracle Instant Client, IBM's ODBC driver
//! for Db2), and the ODBC drivers and data sources the system has.

use std::sync::Mutex;

use platypus_core::DriverKind;
use platypus_driver_odbc::system::{self, SystemOdbc};
use platypus_drivers::{CancellationToken, DriverError, Installed, VendorPackage};
use serde::Serialize;
use tauri::ipc::Channel;

use crate::error::{CommandError, CommandResult};

/// The download in progress, which `cancel_vendor_install` stops.
static INSTALLING: Mutex<Option<CancellationToken>> = Mutex::new(None);

fn installing() -> std::sync::MutexGuard<'static, Option<CancellationToken>> {
    INSTALLING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn driver_error(e: DriverError) -> CommandError {
    let kind = match e {
        DriverError::Cancelled => "cancelled",
        DriverError::ElevationDeclined => "elevation_declined",
        _ => "drivers",
    };
    CommandError::new(kind, e.to_string())
}

fn package_name(driver: DriverKind) -> CommandResult<&'static str> {
    driver.vendor_package().ok_or_else(|| {
        CommandError::new(
            "drivers",
            format!("{} has no client library to install", driver.name()),
        )
    })
}

fn data_dir() -> CommandResult<&'static std::path::Path> {
    platypus_drivers::data_dir()
        .ok_or_else(|| CommandError::new("drivers", "the drivers folder is not set"))
}

#[derive(Debug, Serialize)]
pub struct VendorStatus {
    /// What the app would download here; `None` where it installs nothing.
    package: Option<VendorPackage>,
    /// The library the app downloaded.
    installed: Option<Installed>,
    /// The ODBC driver the system has for the database (Db2), wherever it came from.
    system_driver: Option<String>,
    /// Whether connections can open: the library is there, and registered when it must be.
    ready: bool,
}

/// The ODBC driver the system has for `driver`, if it is reached through ODBC.
fn system_driver(driver: DriverKind) -> CommandResult<Option<String>> {
    if driver == DriverKind::Oracle {
        return Ok(None);
    }
    platypus_driver_odbc::vendor_driver_installed(driver)
        .map_err(|e| CommandError::new("drivers", e.to_string()))
}

#[tauri::command]
pub fn vendor_driver_status(driver: DriverKind) -> CommandResult<VendorStatus> {
    let name = package_name(driver)?;
    let package = platypus_drivers::vendor_package(name);
    let installed = platypus_drivers::installed_vendor(data_dir()?, name);
    let system_driver = system_driver(driver)?;
    let ready = match driver {
        DriverKind::Oracle => installed.is_some(),
        _ => system_driver.is_some(),
    };
    Ok(VendorStatus {
        package,
        installed,
        system_driver,
        ready,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VendorEvent {
    Progress {
        done: u64,
        total: Option<u64>,
    },
    /// Windows asks the user for administrator rights now.
    Registering,
}

/// Downloads the client library of `driver` unless this version is there, then registers
/// it with the system when it is an ODBC driver (Windows asks for administrator rights).
#[tauri::command]
pub async fn install_vendor_driver(
    driver: DriverKind,
    on_event: Channel<VendorEvent>,
) -> CommandResult<VendorStatus> {
    let name = package_name(driver)?;
    let package = platypus_drivers::vendor_package(name).ok_or_else(|| {
        CommandError::new(
            "drivers",
            format!(
                "PlatypusDB does not install the {} client on this system",
                driver.name()
            ),
        )
    })?;
    let dir = data_dir()?;
    let current =
        platypus_drivers::installed_vendor(dir, name).filter(|i| i.version == package.version);
    let installed = if let Some(installed) = current {
        installed
    } else {
        let cancel = CancellationToken::new();
        *installing() = Some(cancel.clone());
        let progress = |done, total| {
            let _ = on_event.send(VendorEvent::Progress { done, total });
        };
        let result = platypus_drivers::install_vendor(dir, &package, progress, &cancel).await;
        installing().take();
        result.map_err(driver_error)?
    };
    if package.register.is_some() && system_driver(driver)?.is_none() {
        let _ = on_event.send(VendorEvent::Registering);
        platypus_drivers::register(&installed, &package)
            .await
            .map_err(driver_error)?;
        if system_driver(driver)?.is_none() {
            return Err(CommandError::new(
                "drivers",
                "the driver was registered, but the ODBC driver manager does not list it",
            ));
        }
    }
    vendor_driver_status(driver)
}

#[tauri::command]
pub fn cancel_vendor_install() {
    if let Some(cancel) = installing().as_ref() {
        cancel.cancel();
    }
}

/// Unregisters the library from the system when the app registered it (as administrator),
/// then deletes it.
#[tauri::command]
pub async fn remove_vendor_driver(driver: DriverKind) -> CommandResult<VendorStatus> {
    let name = package_name(driver)?;
    let dir = data_dir()?;
    if let (Some(installed), Some(package)) = (
        platypus_drivers::installed_vendor(dir, name),
        platypus_drivers::vendor_package(name),
    ) {
        platypus_drivers::unregister(&installed, &package)
            .await
            .map_err(driver_error)?;
    }
    platypus_drivers::remove_vendor(dir, name).map_err(driver_error)?;
    vendor_driver_status(driver)
}

#[derive(Debug, Serialize)]
pub struct OdbcSystem {
    /// Whether this build reaches ODBC drivers at all (Windows).
    available: bool,
    #[serde(flatten)]
    system: SystemOdbc,
}

/// The ODBC drivers and data sources the system's driver manager knows.
#[tauri::command]
pub fn odbc_system() -> CommandResult<OdbcSystem> {
    Ok(OdbcSystem {
        available: system::AVAILABLE,
        system: system::list().map_err(|e| CommandError::new("drivers", e.to_string()))?,
    })
}
