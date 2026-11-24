//! JDBC drivers the app downloads: single jar files, from Maven Central, checked with their
//! SHA-256 and kept under `<data>/drivers/jdbc/`. The JDBC bridge (`platypus-driver-odbc`) loads
//! them; IBM's is the one for Db2 ([`jdbc_driver`]), which a connection reaches without any
//! client installed.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::install::{check, download};
use crate::registry::client;
use crate::{DriverError, drivers_dir, io_error};

/// A JDBC driver the app can download.
#[derive(Debug, Clone, Serialize)]
pub struct JdbcDriver {
    /// `db2jcc`.
    pub name: String,
    pub title: String,
    pub version: String,
    pub license: String,
    pub license_url: String,
    pub vendor_url: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    /// The file name the jar is kept under.
    pub file: String,
}

/// Every JDBC driver the app knows, by name.
pub const JDBC_DRIVERS: &[&str] = &["db2jcc"];

/// IBM's JDBC driver for Db2 (type 4, no client needed), from Maven Central.
#[must_use]
pub fn jdbc_driver(name: &str) -> Option<JdbcDriver> {
    (name == "db2jcc").then(|| JdbcDriver {
        name: "db2jcc".to_owned(),
        title: "IBM Data Server Driver for JDBC and SQLJ".to_owned(),
        version: "12.1.4.0".to_owned(),
        license: "IBM International Program License Agreement".to_owned(),
        license_url:
            "https://www.ibm.com/support/customer/csol/terms/?ref=L-XFLK-S5N8U5-01-03-2026-zz-en"
                .to_owned(),
        vendor_url: "https://repo1.maven.org/maven2/com/ibm/db2/jcc/".to_owned(),
        url: "https://repo1.maven.org/maven2/com/ibm/db2/jcc/12.1.4.0/jcc-12.1.4.0.jar".to_owned(),
        sha256: "9864979950be6550d63d4ceb6e3defa159e850f5f70d35c3dc44b4403606c0b3".to_owned(),
        size: 6_629_661,
        file: "jcc-12.1.4.0.jar".to_owned(),
    })
}

/// The folder the downloaded jars are kept in.
#[must_use]
pub fn jdbc_dir(data_dir: &Path) -> PathBuf {
    drivers_dir(data_dir).join("jdbc")
}

/// The jar of `driver` when it is downloaded.
#[must_use]
pub fn installed_jdbc(data_dir: &Path, driver: &JdbcDriver) -> Option<PathBuf> {
    let path = jdbc_dir(data_dir).join(&driver.file);
    path.is_file().then_some(path)
}

/// Downloads the jar of `driver` and keeps it. `progress` gets the bytes received and the total.
pub async fn install_jdbc(
    data_dir: &Path,
    driver: &JdbcDriver,
    progress: impl Fn(u64, Option<u64>),
    cancel: &CancellationToken,
) -> Result<PathBuf, DriverError> {
    let http = client(driver.url.starts_with("https:"))?;
    let bytes = download(&http, &driver.url, progress, cancel).await?;
    check(&bytes, &driver.sha256)?;
    let dir = jdbc_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(io_error(&dir))?;
    let path = dir.join(&driver.file);
    let tmp = dir.join(format!("{}.{}.part", driver.file, std::process::id()));
    std::fs::write(&tmp, &bytes).map_err(io_error(&tmp))?;
    let _ = std::fs::remove_file(&path);
    std::fs::rename(&tmp, &path).map_err(io_error(&path))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_known_driver_names_its_file_and_checksum() {
        let driver = jdbc_driver("db2jcc").unwrap();
        assert!(driver.url.ends_with(&driver.file));
        assert_eq!(driver.sha256.len(), 64);
        assert!(jdbc_driver("nothing").is_none());
        for name in JDBC_DRIVERS {
            assert!(jdbc_driver(name).is_some());
        }
    }

    #[test]
    fn a_jar_that_is_there_is_installed() {
        let dir = std::env::temp_dir().join(format!("platypus-jdbc-{}", std::process::id()));
        let driver = jdbc_driver("db2jcc").unwrap();
        assert!(installed_jdbc(&dir, &driver).is_none());
        std::fs::create_dir_all(jdbc_dir(&dir)).unwrap();
        std::fs::write(jdbc_dir(&dir).join(&driver.file), b"PK").unwrap();
        assert_eq!(
            installed_jdbc(&dir, &driver),
            Some(jdbc_dir(&dir).join(&driver.file))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
