//! JDBC drivers the app downloads: single jar files, from Maven Central, checked with their
//! SHA-256 and kept under `<data>/drivers/jdbc/`. A connection names the version it uses, as in
//! DBeaver's driver manager, and each version is downloaded once, when a connection first needs it.
//! The JDBC bridge (`platypus-driver-odbc`) loads them; IBM's is the one for Db2
//! ([`jdbc_versions`]), which a connection reaches without any client installed.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::install::{check, download};
use crate::registry::client;
use crate::{DriverError, drivers_dir, io_error};

/// A JDBC driver the app can download, in one version.
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

/// IBM's JDBC driver for Db2 on Maven Central: version, SHA-256 and size, newest first. The
/// checksums were taken from the files and compared with Maven Central's SHA-1.
const DB2JCC: &[(&str, &str, u64)] = &[
    (
        "12.1.5.0",
        "04150111a29370247d162c3f05b8e938bf1314f19a63ad9a5302d88dd478e8e8",
        6_630_167,
    ),
    (
        "12.1.4.0",
        "9864979950be6550d63d4ceb6e3defa159e850f5f70d35c3dc44b4403606c0b3",
        6_629_661,
    ),
    (
        "12.1.3.0",
        "6380b30ba7ce3262c775720066dbd2a3abc14dd0212619068e589866894029c7",
        6_618_519,
    ),
    (
        "12.1.2.0",
        "b19afdd4eb354c933f39bfbe239b21f1a132aa9113d885db73dc573495523dba",
        6_615_134,
    ),
    (
        "12.1.0.0",
        "29a514dd740a20661d6a22f67a3a118e2c11dc14fa4056e4faeb08afb1738fd8",
        6_608_483,
    ),
    (
        "11.5.9.0",
        "3d7ddbfe04b63cc349fe6cc7f5aaf92cf21a1a11dd57225298aa716f9f4ecda4",
        6_547_072,
    ),
];

fn db2jcc(version: &str, sha256: &str, size: u64) -> JdbcDriver {
    let file = format!("jcc-{version}.jar");
    JdbcDriver {
        name: "db2jcc".to_owned(),
        title: "IBM Data Server Driver for JDBC and SQLJ".to_owned(),
        version: version.to_owned(),
        license: "IBM International Program License Agreement".to_owned(),
        license_url:
            "https://www.ibm.com/support/customer/csol/terms/?ref=L-XFLK-S5N8U5-01-03-2026-zz-en"
                .to_owned(),
        vendor_url: "https://repo1.maven.org/maven2/com/ibm/db2/jcc/".to_owned(),
        url: format!("https://repo1.maven.org/maven2/com/ibm/db2/jcc/{version}/{file}"),
        sha256: sha256.to_owned(),
        size,
        file,
    }
}

/// The versions of JDBC driver `name` the app can download, newest first.
#[must_use]
pub fn jdbc_versions(name: &str) -> Vec<JdbcDriver> {
    if name == "db2jcc" {
        DB2JCC.iter().map(|(v, h, s)| db2jcc(v, h, *s)).collect()
    } else {
        Vec::new()
    }
}

/// `version` of JDBC driver `name`, or the newest without one.
#[must_use]
pub fn jdbc_driver(name: &str, version: Option<&str>) -> Option<JdbcDriver> {
    let versions = jdbc_versions(name);
    match version {
        Some(v) => versions.into_iter().find(|d| d.version == v),
        None => versions.into_iter().next(),
    }
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

/// Deletes the downloaded jar of `driver`, if there is one.
pub fn remove_jdbc(data_dir: &Path, driver: &JdbcDriver) -> Result<(), DriverError> {
    let path = jdbc_dir(data_dir).join(&driver.file);
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_error(&path)(e)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_version_names_its_file_and_checksum() {
        let versions = jdbc_versions("db2jcc");
        assert_eq!(versions.len(), 6);
        for d in &versions {
            assert!(d.url.ends_with(&d.file), "{}", d.url);
            assert!(d.url.contains(&format!("/{}/", d.version)));
            assert_eq!(d.sha256.len(), 64);
        }
        assert_eq!(versions[0].version, "12.1.5.0");
        assert!(jdbc_versions("nothing").is_empty());
        for name in JDBC_DRIVERS {
            assert!(jdbc_driver(name, None).is_some());
        }
    }

    #[test]
    fn a_version_is_found_by_its_number() {
        assert_eq!(
            jdbc_driver("db2jcc", Some("11.5.9.0")).unwrap().file,
            "jcc-11.5.9.0.jar"
        );
        assert_eq!(jdbc_driver("db2jcc", None).unwrap().version, "12.1.5.0");
        assert!(jdbc_driver("db2jcc", Some("1.0")).is_none());
    }

    #[test]
    fn a_jar_that_is_there_is_installed_and_can_be_removed() {
        let dir = std::env::temp_dir().join(format!("platypus-jdbc-{}", std::process::id()));
        let driver = jdbc_driver("db2jcc", Some("12.1.4.0")).unwrap();
        assert!(installed_jdbc(&dir, &driver).is_none());
        std::fs::create_dir_all(jdbc_dir(&dir)).unwrap();
        std::fs::write(jdbc_dir(&dir).join(&driver.file), b"PK").unwrap();
        assert_eq!(
            installed_jdbc(&dir, &driver),
            Some(jdbc_dir(&dir).join(&driver.file))
        );
        remove_jdbc(&dir, &driver).unwrap();
        assert!(installed_jdbc(&dir, &driver).is_none());
        remove_jdbc(&dir, &driver).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
