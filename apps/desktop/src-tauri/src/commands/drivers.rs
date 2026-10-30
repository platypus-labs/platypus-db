//! Drivers downloaded when a database needs one (ADBC): what is installed, what the
//! registry offers, the download with its progress, and removal.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use futures::TryStreamExt as _;
use platypus_core::{DriverKind, ExecOptions, ExecResult};
use platypus_drivers::{
    CancellationToken, DriverError, Installed, InstalledVersion, Package, Registry, Update,
};
use platypus_store::SavedConnection;
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;

use super::connections::{PasswordInput, SshSecrets, open_for_test};
use super::{AppState, lock};
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

/// The environment variable that points the app at another driver registry (a mirror, or the
/// fake one the end-to-end tests serve).
const REGISTRY_ENV: &str = "PLATYPUS_DRIVER_REGISTRY";

/// The registry drivers are downloaded from: the public one unless `PLATYPUS_DRIVER_REGISTRY`
/// names another.
fn registry(dir: &Path) -> Registry {
    match std::env::var(REGISTRY_ENV) {
        Ok(url) if !url.trim().is_empty() => Registry::with_url(url.trim(), dir),
        _ => Registry::public(dir),
    }
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
    let (package, error) = match registry(dir).package(name).await {
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

/// Downloads and installs `package`, reporting progress on `on_event`; the download can be
/// cancelled by package name.
async fn install_package(
    name: &'static str,
    registry: &Registry,
    package: &Package,
    on_event: &Channel<InstallEvent>,
) -> CommandResult<Installed> {
    let (cancel, _installing) = start_install(name);
    let progress = |done, total| {
        let _ = on_event.send(InstallEvent::Progress { done, total });
    };
    registry
        .install(package, progress, &cancel)
        .await
        .map_err(driver_error)
}

/// Downloads and installs the driver of `driver`, reporting progress on `on_event`.
#[tauri::command]
pub async fn install_driver(
    driver: DriverKind,
    on_event: Channel<InstallEvent>,
) -> CommandResult<Installed> {
    let name = package_name(driver)?;
    let registry = registry(data_dir()?);
    let package = registry.package(name).await.map_err(driver_error)?;
    install_package(name, &registry, &package, &on_event).await
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

/// The installed and the available versions of one driver.
#[derive(Debug, Serialize)]
pub struct DriverVersions {
    /// Every version on disk, newest first; one is active.
    installed: Vec<InstalledVersion>,
    /// What the registry has for this platform, newest first; empty when it cannot be reached.
    available: Vec<Package>,
    /// Why `available` is empty.
    error: Option<String>,
}

/// The versions of the driver of `driver`: installed and in the registry.
#[tauri::command]
pub async fn driver_versions(driver: DriverKind) -> CommandResult<DriverVersions> {
    let name = package_name(driver)?;
    let dir = data_dir()?;
    let (available, error) = match registry(dir).packages(name).await {
        Ok(p) => (p, None),
        Err(e) => (Vec::new(), Some(e.to_string())),
    };
    Ok(DriverVersions {
        installed: platypus_drivers::versions(dir, name),
        available,
        error,
    })
}

/// Downloads `version` of the driver of `driver`, checks it and makes it the active one; the
/// versions already installed stay, so the app can go back to them.
#[tauri::command]
pub async fn install_driver_version(
    driver: DriverKind,
    version: String,
    on_event: Channel<InstallEvent>,
) -> CommandResult<Installed> {
    let name = package_name(driver)?;
    let registry = registry(data_dir()?);
    let package = registry
        .packages(name)
        .await
        .map_err(driver_error)?
        .into_iter()
        .find(|p| p.version == version)
        .ok_or_else(|| {
            CommandError::new("drivers", format!("the registry has no {name} {version}"))
        })?;
    install_package(name, &registry, &package, &on_event).await
}

/// Makes the installed `version` of the driver of `driver` the one connections use. Sessions
/// already open keep the library they loaded; new ones load this version.
#[tauri::command]
pub fn activate_driver_version(driver: DriverKind, version: String) -> CommandResult<Installed> {
    platypus_drivers::activate(data_dir()?, package_name(driver)?, &version).map_err(driver_error)
}

/// Deletes one installed version of the driver of `driver`; returns the ones left. The active
/// version is kept while a session of that database is open, as its library is loaded.
#[tauri::command]
pub fn remove_driver_version(
    state: State<'_, AppState>,
    driver: DriverKind,
    version: String,
) -> CommandResult<Vec<InstalledVersion>> {
    let name = package_name(driver)?;
    let dir = data_dir()?;
    let active = platypus_drivers::versions(dir, name)
        .into_iter()
        .any(|v| v.active && v.installed.version == version);
    if active && sessions_using(&state, driver) > 0 {
        return Err(CommandError::new(
            "drivers",
            format!(
                "{} {version} is in use by an open connection: close it or activate another version first",
                driver.name()
            ),
        ));
    }
    platypus_drivers::remove_version(dir, name, &version).map_err(driver_error)?;
    Ok(platypus_drivers::versions(dir, name))
}

/// How many open sessions load the library of `driver`'s downloaded driver.
fn sessions_using(state: &AppState, driver: DriverKind) -> usize {
    lock(&state.sessions)
        .values()
        .filter(|s| s.config.driver == driver)
        .count()
}

/// The result of asking the registry for newer versions of the installed drivers.
#[derive(Debug, Serialize)]
pub struct UpdateCheck {
    /// Whether the registry was asked: a check that is not forced and ran less than a day ago
    /// does not, and answers with the updates the last one found.
    checked: bool,
    updates: Vec<Update>,
    /// Why the registry could not answer.
    error: Option<String>,
}

/// Looks for newer versions of the installed drivers (their active versions). Unless `force`,
/// it asks the registry at most once a day.
#[tauri::command]
pub async fn check_driver_updates(force: bool) -> CommandResult<UpdateCheck> {
    let dir = data_dir()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    if !force && !platypus_drivers::update_check_due(platypus_drivers::last_update_check(dir), now)
    {
        return Ok(UpdateCheck {
            checked: false,
            updates: platypus_drivers::stored_updates(dir),
            error: None,
        });
    }
    match registry(dir).updates(&platypus_drivers::list(dir)).await {
        Ok(updates) => {
            let _ = platypus_drivers::record_update_check(dir, now);
            let _ = platypus_drivers::store_updates(dir, &updates);
            Ok(UpdateCheck {
                checked: true,
                updates,
                error: None,
            })
        }
        Err(e) => Ok(UpdateCheck {
            checked: true,
            updates: Vec::new(),
            error: Some(e.to_string()),
        }),
    }
}

/// What a saved connection answered to the compatibility probe.
#[derive(Debug, Serialize)]
pub struct Probe {
    product: String,
    version: String,
    /// The schemas the session lists.
    schemas: usize,
}

/// Opens `connection` with the password it keeps and asks it the little the app always asks:
/// the server's version, its schemas, and `SELECT 1`. Run after a driver changes version to
/// see whether the connections that use it still work.
#[tauri::command]
pub async fn probe_connection(
    state: State<'_, AppState>,
    connection: SavedConnection,
) -> CommandResult<Probe> {
    let conn = open_for_test(&state, &connection, PasswordInput::Keep, SshSecrets::new()).await?;
    let server = conn.server_info().clone();
    let schemas = conn.schemas().await?.len();
    match conn.execute("SELECT 1", &ExecOptions::default()).await? {
        ExecResult::Rows(stream) => stream.try_for_each(|_| async { Ok(()) }).await?,
        ExecResult::Affected { .. } => {}
    }
    Ok(Probe {
        product: server.product,
        version: server.version,
        schemas,
    })
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
