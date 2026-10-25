//! Drivers downloaded when a database needs one (ADBC): what is installed, what the
//! registry offers, the download with its progress, and removal.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use platypus_core::DriverKind;
use platypus_drivers::{CancellationToken, DriverError, Installed, Package, Registry};
use serde::Serialize;
use tauri::ipc::Channel;

use crate::error::{CommandError, CommandResult};

/// The downloads in progress by package name (the vendors' clients too), which
/// `cancel_driver_install` and `cancel_vendor_install` stop. Each has an id, so a download
/// that ends does not forget a newer one of the same driver.
static INSTALLING: Mutex<BTreeMap<&'static str, (u64, CancellationToken)>> =
    Mutex::new(BTreeMap::new());
static NEXT_INSTALL: AtomicU64 = AtomicU64::new(0);

fn installing() -> MutexGuard<'static, BTreeMap<&'static str, (u64, CancellationToken)>> {
    INSTALLING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Records the download of `name`; it is forgotten when the returned guard drops.
pub(super) fn start_install(name: &'static str) -> (CancellationToken, InstallGuard) {
    let id = NEXT_INSTALL.fetch_add(1, Ordering::Relaxed);
    let cancel = CancellationToken::new();
    installing().insert(name, (id, cancel.clone()));
    (cancel, InstallGuard { name, id })
}

pub(super) struct InstallGuard {
    name: &'static str,
    id: u64,
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        let mut all = installing();
        if all.get(self.name).is_some_and(|(id, _)| *id == self.id) {
            all.remove(self.name);
        }
    }
}

/// Stops the download of `name`, if one is running.
pub(super) fn cancel_install(name: &str) {
    if let Some((_, cancel)) = installing().get(name) {
        cancel.cancel();
    }
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
    let (cancel, _installing) = start_install(name);
    let progress = |done, total| {
        let _ = on_event.send(InstallEvent::Progress { done, total });
    };
    registry
        .install(&package, progress, &cancel)
        .await
        .map_err(driver_error)
}

/// Stops the download of `driver`'s driver; others go on.
#[tauri::command]
pub fn cancel_driver_install(driver: DriverKind) -> CommandResult<()> {
    cancel_install(package_name(driver)?);
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancels_only_the_named_download() {
        let (a, guard_a) = start_install("test-a");
        let (b, _guard_b) = start_install("test-b");
        cancel_install("test-a");
        assert!(a.is_cancelled());
        assert!(!b.is_cancelled());
        drop(guard_a);
        assert!(!installing().contains_key("test-a"));
        assert!(installing().contains_key("test-b"));
    }

    #[test]
    fn a_finished_download_keeps_a_newer_one() {
        let (_, old) = start_install("test-c");
        let (newer, _guard) = start_install("test-c");
        drop(old);
        cancel_install("test-c");
        assert!(newer.is_cancelled());
    }
}
