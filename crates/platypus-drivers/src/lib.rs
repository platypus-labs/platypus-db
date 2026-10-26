//! Drivers that PlatypusDB downloads when a connection needs them, instead of shipping them:
//! ADBC drivers from the `dbc` driver registry (Columnar), kept under
//! `<data>/drivers/adbc/<name>/<version>/` with a `driver.toml` that names the library, and
//! the vendors' client libraries (Oracle Instant Client, IBM's ODBC driver) under
//! `<data>/drivers/vendor/` ([`vendor_package`]).

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

mod install;
mod registry;
mod vendor;

pub use registry::{Package, Registry};
pub use tokio_util::sync::CancellationToken;
pub use vendor::{
    Archive, Program, VENDOR_PACKAGES, VendorPackage, install_vendor, installed_vendor, register,
    remove_vendor, unregister, vendor_package,
};

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Sets the folder that holds the app's data, where drivers are kept; once, at start.
pub fn set_data_dir(dir: &Path) {
    let _ = DATA_DIR.set(dir.to_owned());
}

/// The folder set by [`set_data_dir`].
#[must_use]
pub fn data_dir() -> Option<&'static Path> {
    DATA_DIR.get().map(PathBuf::as_path)
}

/// The registry `dbc` installs from.
pub const PUBLIC_REGISTRY: &str = "https://dbc-cdn.columnar.tech";

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("cannot reach the driver registry: {0}")]
    Network(String),
    #[error("no {name} driver for this platform ({platform})")]
    NoPackage { name: String, platform: String },
    #[error("the driver's license ({0}) does not allow downloading it here")]
    License(String),
    #[error("the downloaded driver is corrupted (its SHA-256 does not match)")]
    Checksum,
    #[error("the registry gives no SHA-256 for this driver, so the download cannot be checked")]
    NoChecksum,
    #[error("`{0}` is not a valid driver name")]
    InvalidName(String),
    #[error("the driver package holds a file larger than allowed")]
    TooLarge,
    #[error("cancelled")]
    Cancelled,
    #[error(
        "the driver was not registered: Windows asks for administrator rights to register it, \
         and the request was declined"
    )]
    ElevationDeclined,
    #[error("cannot register the driver: {0}")]
    Register(String),
    #[error("invalid driver package: {0}")]
    Format(String),
    #[error("cannot write {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub(crate) fn network_error(e: impl std::fmt::Display) -> DriverError {
    DriverError::Network(e.to_string())
}

pub(crate) fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> DriverError + '_ {
    move |source| DriverError::Io {
        path: path.to_owned(),
        source,
    }
}

/// A driver downloaded and ready to load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    /// Its name in the registry, e.g. `snowflake`.
    pub name: String,
    pub title: String,
    pub version: String,
    pub license: String,
    /// The shared library.
    pub library: PathBuf,
}

/// The registry's name for this operating system and processor, e.g. `windows_amd64`.
#[must_use]
pub fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "windows_arm64",
        ("windows", _) => "windows_amd64",
        ("macos", "aarch64") => "macos_arm64",
        ("macos", _) => "macos_amd64",
        (_, "aarch64") => "linux_arm64",
        _ => "linux_amd64",
    }
}

pub(crate) fn drivers_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("drivers")
}

pub(crate) fn adbc_dir(data_dir: &Path) -> PathBuf {
    drivers_dir(data_dir).join("adbc")
}

/// The file a removal could not delete yet: Windows keeps a loaded library locked.
const REMOVED_MARK: &str = ".removed";

/// Whether `name` is one plain folder name (no `..`, separators or drive), so joining it
/// stays inside the drivers' folder.
pub(crate) fn is_plain_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(n)), None) if n == name
    )
}

/// The installed driver `name`, the newest if several versions are there.
#[must_use]
pub fn installed(data_dir: &Path, name: &str) -> Option<Installed> {
    installed_in(&adbc_dir(data_dir), name)
}

/// The newest version of `name` under `root`, unless it was removed.
pub(crate) fn installed_in(root: &Path, name: &str) -> Option<Installed> {
    if !is_plain_name(name) {
        return None;
    }
    let dir = root.join(name);
    if dir.join(REMOVED_MARK).exists() {
        return None;
    }
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| read_installed(&e.path()))
        .max_by(|a, b| registry::compare_versions(&a.version, &b.version))
}

fn read_installed(dir: &Path) -> Option<Installed> {
    let text = std::fs::read_to_string(dir.join("driver.toml")).ok()?;
    let mut installed: Installed = toml::from_str(&text).ok()?;
    // Stored relative, so the data folder can move.
    installed.library = dir.join(&installed.library);
    installed.library.is_file().then_some(installed)
}

/// Every installed driver, by name.
#[must_use]
pub fn list(data_dir: &Path) -> Vec<Installed> {
    let mut all: Vec<Installed> = std::fs::read_dir(adbc_dir(data_dir))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| installed(data_dir, &e.file_name().to_string_lossy()))
        .collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all
}

/// Deletes an installed driver. A library still loaded cannot be deleted on Windows: the
/// driver is marked removed, and [`purge_removed`] deletes it at the next start.
pub fn remove(data_dir: &Path, name: &str) -> Result<(), DriverError> {
    remove_in(&adbc_dir(data_dir), name)
}

/// Deletes `root/name`, or marks it removed when Windows keeps its files locked.
pub(crate) fn remove_in(root: &Path, name: &str) -> Result<(), DriverError> {
    if !is_plain_name(name) {
        return Err(DriverError::InvalidName(name.to_owned()));
    }
    let dir = root.join(name);
    if !dir.exists() {
        return Ok(());
    }
    if std::fs::remove_dir_all(&dir).is_err() {
        std::fs::write(dir.join(REMOVED_MARK), b"").map_err(io_error(&dir))?;
    }
    Ok(())
}

/// Deletes the drivers marked removed and downloads left half done; called at start.
pub fn purge_removed(data_dir: &Path) {
    let root = drivers_dir(data_dir);
    for entry in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(install::TMP_PREFIX)
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    purge_marked(&adbc_dir(data_dir));
    vendor::purge_removed_vendor(data_dir);
}

/// Deletes the folders under `root` marked removed.
pub(crate) fn purge_marked(root: &Path) {
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        if entry.path().join(REMOVED_MARK).exists() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests;
