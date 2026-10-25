//! Loading Oracle's client library: the Instant Client the app downloaded, or else one the
//! system has. ODPI-C loads it once per process.

use std::path::Path;
use std::sync::Mutex;

use platypus_core::{DriverKind, Error, Result};

/// Whether the client library is loaded; a failed load is tried again next time.
static LOADED: Mutex<bool> = Mutex::new(false);

/// The Visual C++ runtime files Instant Client needs on Windows.
const VC_RUNTIME: &[&str] = &["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll"];

/// Loads the client library: the Instant Client under `data_dir`, or the system's when the
/// app has not downloaded one. Without either, the driver is missing.
pub fn load(data_dir: &Path) -> Result<()> {
    let mut loaded = LOADED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *loaded {
        return Ok(());
    }
    let installed = platypus_drivers::installed_vendor(data_dir, "oracle");
    let mut params = oracle::InitParams::new();
    let config_error = |e: oracle::Error| Error::Config(e.to_string());
    params
        .default_driver_name("PlatypusDB")
        .map_err(config_error)?;
    let dir = installed
        .as_ref()
        .and_then(platypus_drivers::VendorPackage::library_dir);
    if let Some(dir) = dir {
        params.oracle_client_lib_dir(dir).map_err(config_error)?;
    }
    match params.init() {
        Ok(_) => {
            *loaded = true;
            Ok(())
        }
        // No client of the app's nor of the system's: offer the download.
        Err(_) if dir.is_none() => Err(Error::DriverMissing {
            driver: DriverKind::Oracle,
        }),
        Err(e) => Err(Error::Connect(load_error(
            &e.to_string(),
            std::env::consts::OS,
            system_has,
        ))),
    }
}

/// Whether the Windows system folder holds `file`.
fn system_has(file: &str) -> bool {
    std::env::var_os("SystemRoot")
        .is_some_and(|root| Path::new(&root).join("System32").join(file).is_file())
}

/// Why the downloaded client did not load, in words the user can act on: the Visual C++
/// runtime missing on Windows, `libaio` on Linux.
pub fn load_error(error: &str, os: &str, system_has: impl Fn(&str) -> bool) -> String {
    match os {
        "windows" if VC_RUNTIME.iter().any(|f| !system_has(f)) => format!(
            "Oracle Instant Client needs the Microsoft Visual C++ Redistributable for Visual \
             Studio 2015–2022 (x64), which is not installed. Install it from \
             https://aka.ms/vs/17/release/vc_redist.x64.exe, then restart PlatypusDB. ({error})"
        ),
        "linux" if error.contains("libaio") => format!(
            "Oracle Instant Client needs the libaio library: install the `libaio1` (or \
             `libaio1t64`, or `libaio`) package. ({error})"
        ),
        _ => format!("cannot load Oracle Instant Client: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_load_failures() {
        let dpi = "DPI-1047: Cannot locate a 64-bit Oracle Client library";
        let message = load_error(dpi, "windows", |f| f != "msvcp140.dll");
        assert!(message.contains("Visual C++ Redistributable"), "{message}");
        assert!(message.contains("vc_redist.x64.exe"));
        assert!(message.ends_with(&format!("({dpi})")));
        let message = load_error(dpi, "windows", |_| true);
        assert!(
            message.starts_with("cannot load Oracle Instant Client"),
            "{message}"
        );
        let message = load_error(
            "libaio.so.1: cannot open shared object file",
            "linux",
            |_| true,
        );
        assert!(message.contains("libaio1"), "{message}");
    }
}
