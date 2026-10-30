//! Several versions of one downloaded driver side by side: which is active, switching,
//! removing one, finding updates, and the daily rhythm of the update check. A driver's
//! folder is `<name>/<version>/…`; the version named in `<name>/ACTIVE` is the one the app
//! loads, and without that file (installs from before versions could be chosen) the newest.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::registry::compare_versions;
use crate::{
    DriverError, Installed, Package, REMOVED_MARK, adbc_dir, drivers_dir, io_error, is_plain_name,
    read_installed,
};

/// Names the version in use among those of a driver's folder.
const ACTIVE_FILE: &str = "ACTIVE";

/// The file that holds what the last check found, so the updates survive a restart.
const UPDATES_FILE: &str = "last-updates.toml";

/// The file that holds when the registry was last asked for updates.
const CHECK_FILE: &str = "last-update-check";

/// The registry is asked for updates at most this often by itself.
const CHECK_EVERY_SECS: u64 = 24 * 60 * 60;

/// A version of a driver that is installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledVersion {
    #[serde(flatten)]
    pub installed: Installed,
    /// The one the app loads.
    pub active: bool,
}

/// A newer version in the registry than the active one installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Update {
    pub name: String,
    /// The version in use now.
    pub installed: String,
    /// The package that would replace it.
    pub package: Package,
}

/// Every installed version of `name` under `root`, newest first.
pub(crate) fn versions_in(root: &Path, name: &str) -> Vec<Installed> {
    if !is_plain_name(name) {
        return Vec::new();
    }
    let dir = root.join(name);
    if dir.join(REMOVED_MARK).exists() {
        return Vec::new();
    }
    let mut all: Vec<Installed> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        // A version removed while Windows kept its library locked waits for the next start.
        .filter(|e| !e.path().join(REMOVED_MARK).exists())
        .filter_map(|e| read_installed(&e.path()))
        .collect();
    all.sort_by(|a, b| compare_versions(&b.version, &a.version));
    all
}

/// The version of `name` the app loads: the chosen one if it is still there, else the newest.
pub(crate) fn active_in(root: &Path, name: &str) -> Option<Installed> {
    let all = versions_in(root, name);
    let chosen = std::fs::read_to_string(root.join(name).join(ACTIVE_FILE)).ok();
    let chosen = chosen.as_deref().map(str::trim);
    let index = chosen
        .and_then(|v| all.iter().position(|i| i.version == v))
        .unwrap_or(0);
    all.into_iter().nth(index)
}

/// Every installed version of `name`, newest first, with the active one marked.
#[must_use]
pub fn versions(data_dir: &Path, name: &str) -> Vec<InstalledVersion> {
    let root = adbc_dir(data_dir);
    let active = active_in(&root, name).map(|i| i.version);
    versions_in(&root, name)
        .into_iter()
        .map(|installed| InstalledVersion {
            active: active.as_deref() == Some(installed.version.as_str()),
            installed,
        })
        .collect()
}

/// Fails unless the driver name and the version are plain folder names.
fn check_names(name: &str, version: &str) -> Result<(), DriverError> {
    for part in [name, version] {
        if !is_plain_name(part) {
            return Err(DriverError::InvalidName(part.to_owned()));
        }
    }
    Ok(())
}

/// Makes `version` of `name` the one the app loads; the other versions stay.
pub fn activate(data_dir: &Path, name: &str, version: &str) -> Result<Installed, DriverError> {
    check_names(name, version)?;
    let root = adbc_dir(data_dir);
    let found = versions_in(&root, name)
        .into_iter()
        .find(|i| i.version == version)
        .ok_or_else(|| DriverError::Format(format!("{name} {version} is not installed")))?;
    set_active(&root.join(name), version)?;
    Ok(found)
}

/// Records `version` as the active one of the driver folder `dir`.
pub(crate) fn set_active(dir: &Path, version: &str) -> Result<(), DriverError> {
    let file = dir.join(ACTIVE_FILE);
    std::fs::write(&file, version).map_err(io_error(&file))
}

/// Deletes one installed version. When it was the active one, the newest left takes over.
/// A library Windows keeps locked is marked removed and deleted at the next start.
pub fn remove_version(data_dir: &Path, name: &str, version: &str) -> Result<(), DriverError> {
    check_names(name, version)?;
    let root = adbc_dir(data_dir);
    let dir = root.join(name).join(version);
    if !dir.exists() {
        return Ok(());
    }
    if versions_in(&root, name).len() <= 1 {
        return crate::remove_in(&root, name);
    }
    if std::fs::remove_dir_all(&dir).is_err() {
        std::fs::write(dir.join(REMOVED_MARK), b"").map_err(io_error(&dir))?;
    }
    Ok(())
}

/// Moves a driver installed before versions had their own folder (files straight in
/// `<name>/`, with the `driver.toml` that names the version) into `<name>/<version>/`, so it
/// is not downloaded again.
pub fn migrate_layout(data_dir: &Path) {
    for entry in std::fs::read_dir(adbc_dir(data_dir))
        .into_iter()
        .flatten()
        .flatten()
    {
        migrate_one(&entry.path());
    }
}

fn migrate_one(dir: &Path) {
    let Some(installed) = read_installed(dir) else {
        return;
    };
    if !is_plain_name(&installed.version) {
        return;
    }
    let target = dir.join(&installed.version);
    if std::fs::create_dir_all(&target).is_err() {
        return;
    }
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let keep = name == ACTIVE_FILE || name == REMOVED_MARK;
        if entry.path().is_file() && !keep {
            let _ = std::fs::rename(entry.path(), target.join(name));
        }
    }
    let _ = set_active(dir, &installed.version);
}

/// The newest of `packages` (any order) when it is newer than `installed`.
#[must_use]
pub fn newer_than<'a>(installed: &str, packages: &'a [Package]) -> Option<&'a Package> {
    packages
        .iter()
        .max_by(|a, b| compare_versions(&a.version, &b.version))
        .filter(|p| compare_versions(&p.version, installed).is_gt())
}

/// Whether the registry should be asked for updates by itself: never asked, a day since, or
/// the clock went back.
#[must_use]
pub fn update_check_due(last: Option<u64>, now: u64) -> bool {
    last.is_none_or(|last| now < last || now - last >= CHECK_EVERY_SECS)
}

fn check_file(data_dir: &Path) -> PathBuf {
    drivers_dir(data_dir).join(CHECK_FILE)
}

/// When the registry was last asked for updates (seconds since the epoch).
#[must_use]
pub fn last_update_check(data_dir: &Path) -> Option<u64> {
    std::fs::read_to_string(check_file(data_dir))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Notes that the registry was asked for updates at `now`.
pub fn record_update_check(data_dir: &Path, now: u64) -> Result<(), DriverError> {
    let file = check_file(data_dir);
    std::fs::create_dir_all(drivers_dir(data_dir)).map_err(io_error(&file))?;
    std::fs::write(&file, now.to_string()).map_err(io_error(&file))
}

/// What the file of the last check's result holds (TOML needs a table at the top).
#[derive(Serialize, Deserialize, Default)]
struct StoredUpdates {
    #[serde(default)]
    updates: Vec<Update>,
}

/// Keeps what the last check found, replacing the previous result.
pub fn store_updates(data_dir: &Path, updates: &[Update]) -> Result<(), DriverError> {
    let file = drivers_dir(data_dir).join(UPDATES_FILE);
    std::fs::create_dir_all(drivers_dir(data_dir)).map_err(io_error(&file))?;
    let text = toml::to_string(&StoredUpdates {
        updates: updates.to_vec(),
    })
    .map_err(|e| DriverError::Format(e.to_string()))?;
    std::fs::write(&file, text).map_err(io_error(&file))
}

/// The updates the last check found that still apply: those whose driver is still at the
/// version the check saw (one installed or switched since no longer needs its update).
#[must_use]
pub fn stored_updates(data_dir: &Path) -> Vec<Update> {
    let Some(stored) = std::fs::read_to_string(drivers_dir(data_dir).join(UPDATES_FILE))
        .ok()
        .and_then(|text| toml::from_str::<StoredUpdates>(&text).ok())
    else {
        return Vec::new();
    };
    let root = adbc_dir(data_dir);
    stored
        .updates
        .into_iter()
        .filter(|u| active_in(&root, &u.name).is_some_and(|i| i.version == u.installed))
        .collect()
}
