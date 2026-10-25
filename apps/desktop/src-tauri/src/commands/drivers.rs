//! Drivers downloaded when a database needs one (ADBC): what is installed, what the
//! registry offers, the download with its progress, and removal.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use platypus_core::DriverKind;
use platypus_drivers::{CancellationToken, DriverError, Installed, Package, Registry};
use serde::Serialize;
use tauri::ipc::Channel;

use crate::error::{CommandError, CommandResult};

/// The download in progress, which `cancel_driver_install` stops.
static INSTALLING: Mutex<Option<CancellationToken>> = Mutex::new(None);

fn installing() -> MutexGuard<'static, Option<CancellationToken>> {
    INSTALLING.lock().unwrap_or_else(PoisonError::into_inner)
}

fn data_dir() -> CommandResult<&'static Path> {
    platypus_drivers::data_dir()
        .ok_or_else(|| CommandError::new("drivers", "the drivers folder is not set"))
}

fn driver_error(e: DriverError) -> CommandError {
    let kind = if matches!(e, DriverError::Cancelled) {
        "cancelled"
    } else {
        "drivers"
    };
    CommandError::new(kind, e.to_string())
}

fn package_name(driver: DriverKind) -> CommandResult<&'static str> {
    driver.adbc_package().ok_or_else(|| {
        CommandError::new(
            "drivers",
            format!("{} has no driver to download", driver.name()),
        )
    })
}

#[derive(Debug, Serialize)]
pub struct DriverStatus {
    installed: Option<Installed>,
    /// The newest package for this platform; `None` when the registry cannot be reached.
    package: Option<Package>,
    /// Why `package` is missing.
    error: Option<String>,
}

/// What is installed for `driver`, and what the registry would download.
#[tauri::command]
pub async fn driver_status(driver: DriverKind) -> CommandResult<DriverStatus> {
    let name = package_name(driver)?;
    let dir = data_dir()?;
    let (package, error) = match Registry::public(dir).package(name).await {
        Ok(p) => (Some(p), None),
        Err(e) => (None, Some(e.to_string())),
    };
    Ok(DriverStatus {
        installed: platypus_drivers::installed(dir, name),
        package,
        error,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallEvent {
    Progress { done: u64, total: Option<u64> },
}

/// Downloads and installs the driver of `driver`, reporting progress on `on_event`.
#[tauri::command]
pub async fn install_driver(
    driver: DriverKind,
    on_event: Channel<InstallEvent>,
) -> CommandResult<Installed> {
    let name = package_name(driver)?;
    let registry = Registry::public(data_dir()?);
    let package = registry.package(name).await.map_err(driver_error)?;
    let cancel = CancellationToken::new();
    *installing() = Some(cancel.clone());
    let progress = |done, total| {
        let _ = on_event.send(InstallEvent::Progress { done, total });
    };
    let result = registry.install(&package, progress, &cancel).await;
    installing().take();
    result.map_err(driver_error)
}

#[tauri::command]
pub fn cancel_driver_install() {
    if let Some(cancel) = installing().as_ref() {
        cancel.cancel();
    }
}

#[tauri::command]
pub fn list_drivers() -> CommandResult<Vec<Installed>> {
    Ok(platypus_drivers::list(data_dir()?))
}

/// Removes a downloaded driver; returns the ones left.
#[tauri::command]
pub fn remove_driver(name: String) -> CommandResult<Vec<Installed>> {
    let dir = data_dir()?;
    platypus_drivers::remove(dir, &name).map_err(driver_error)?;
    Ok(platypus_drivers::list(dir))
}
