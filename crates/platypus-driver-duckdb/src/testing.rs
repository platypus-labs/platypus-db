//! DuckDB for tests: the library downloaded once under `target/test-drivers`, shared by every
//! test process of the workspace, and loaded. `PLATYPUS_DUCKDB_LIBRARY` names a library to
//! use instead, for working offline.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::library::{self, PACKAGE};

/// Where the tests keep the downloaded library.
fn drivers_dir() -> PathBuf {
    let target = option_env!("CARGO_TARGET_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
        PathBuf::from,
    );
    target.join("test-drivers")
}

/// Loads DuckDB, downloading it first when no test has yet.
///
/// # Panics
///
/// When the library cannot be downloaded or loaded: the test cannot run.
pub fn load() {
    if let Some(path) = std::env::var_os("PLATYPUS_DUCKDB_LIBRARY") {
        library::load_from(Path::new(&path)).expect("PLATYPUS_DUCKDB_LIBRARY");
        return;
    }
    let dir = drivers_dir();
    let installed = platypus_drivers::installed_vendor(&dir, PACKAGE)
        .map_or_else(|| install(&dir), |i| i.library);
    library::load_from(&installed).expect("cannot load the downloaded DuckDB");
}

/// Downloads the package, or waits for the test process downloading it: the first to create
/// the lock file installs.
fn install(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).expect("cannot create target/test-drivers");
    let lock = dir.join("duckdb.lock");
    let stale = |lock: &Path| {
        std::fs::metadata(lock)
            .and_then(|m| m.modified())
            .is_ok_and(|t| {
                SystemTime::now().duration_since(t).unwrap_or_default() > Duration::from_secs(600)
            })
    };
    loop {
        if let Some(installed) = platypus_drivers::installed_vendor(dir, PACKAGE) {
            return installed.library;
        }
        if stale(&lock) {
            let _ = std::fs::remove_file(&lock);
        }
        if std::fs::File::create_new(&lock).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let package = platypus_drivers::vendor_package(PACKAGE).expect("no DuckDB for this platform");
    let owned = dir.to_owned();
    // A runtime of its own, on a thread of its own: the caller may be inside a test's runtime.
    let installed = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
            .block_on(platypus_drivers::install_vendor(
                &owned,
                &package,
                |_, _| {},
                &platypus_drivers::CancellationToken::new(),
            ))
    })
    .join()
    .expect("the download panicked");
    let _ = std::fs::remove_file(&lock);
    installed
        .expect("cannot download DuckDB for the tests")
        .library
}
