//! JDBC drivers the app downloads: jars from Maven Central, checked with their SHA-256 and kept
//! under `<data>/drivers/jdbc/`. A connection names the driver and the version it uses, as in
//! DBeaver's driver manager, and each version is downloaded once, when a connection first needs
//! it. A version is one jar, or several when the driver depends on others (Jaybird, Derby's
//! client). The JDBC bridge (`platypus-driver-odbc`) loads them; IBM's is the one for Db2, which
//! a connection reaches without any client installed.
//!
//! The versions, checksums and sizes were taken from the files themselves, downloaded from Maven
//! Central, and compared with Maven's `.sha1`; the dependencies from each version's POM (only
//! the ones needed at run time). Every driver registers itself with `DriverManager` (JDBC 4) and
//! needs no native library, which is what the bridge relies on.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::install::{check, download};
use crate::registry::client;
use crate::{DriverError, drivers_dir, io_error};

const MAVEN: &str = "https://repo1.maven.org/maven2/";

/// A file of a JDBC driver, downloaded from Maven Central.
#[derive(Debug, Clone, Serialize)]
pub struct JdbcFile {
    /// The file name the jar is kept under.
    pub file: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// A JDBC driver the app can download, in one version.
#[derive(Debug, Clone, Serialize)]
pub struct JdbcDriver {
    /// `h2`, `db2jcc`…
    pub name: String,
    pub title: String,
    pub version: String,
    pub license: String,
    pub license_url: String,
    /// Whether the license is an open source one: otherwise the user reads it before downloading.
    pub open_source: bool,
    pub vendor_url: String,
    /// The JDBC URL with `{host}`, `{port}` and `{database}` where the connection's fields go.
    pub url_template: String,
    pub default_port: Option<u16>,
    /// Every jar of the driver: all of them go on the class path.
    pub files: Vec<JdbcFile>,
}

impl JdbcDriver {
    /// The bytes to download.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// A version: its number and its files as (Maven path, SHA-256, size).
type Release = (&'static str, &'static [(&'static str, &'static str, u64)]);

struct Catalog {
    name: &'static str,
    title: &'static str,
    license: &'static str,
    license_url: &'static str,
    open_source: bool,
    vendor_url: &'static str,
    url_template: &'static str,
    default_port: Option<u16>,
    /// Newest first.
    releases: &'static [Release],
}

/// Every JDBC driver the app knows, by name.
pub const JDBC_DRIVERS: &[&str] = &[
    "db2jcc", "h2", "hsqldb", "firebird", "derby", "hana", "exasol", "vertica",
];

const DB2JCC: &[Release] = &[
    (
        "12.1.5.0",
        &[(
            "com/ibm/db2/jcc/12.1.5.0/jcc-12.1.5.0.jar",
            "04150111a29370247d162c3f05b8e938bf1314f19a63ad9a5302d88dd478e8e8",
            6_630_167,
        )],
    ),
    (
        "12.1.4.0",
        &[(
            "com/ibm/db2/jcc/12.1.4.0/jcc-12.1.4.0.jar",
            "9864979950be6550d63d4ceb6e3defa159e850f5f70d35c3dc44b4403606c0b3",
            6_629_661,
        )],
    ),
    (
        "12.1.3.0",
        &[(
            "com/ibm/db2/jcc/12.1.3.0/jcc-12.1.3.0.jar",
            "6380b30ba7ce3262c775720066dbd2a3abc14dd0212619068e589866894029c7",
            6_618_519,
        )],
    ),
    (
        "12.1.2.0",
        &[(
            "com/ibm/db2/jcc/12.1.2.0/jcc-12.1.2.0.jar",
            "b19afdd4eb354c933f39bfbe239b21f1a132aa9113d885db73dc573495523dba",
            6_615_134,
        )],
    ),
    (
        "12.1.0.0",
        &[(
            "com/ibm/db2/jcc/12.1.0.0/jcc-12.1.0.0.jar",
            "29a514dd740a20661d6a22f67a3a118e2c11dc14fa4056e4faeb08afb1738fd8",
            6_608_483,
        )],
    ),
    (
        "11.5.9.0",
        &[(
            "com/ibm/db2/jcc/11.5.9.0/jcc-11.5.9.0.jar",
            "3d7ddbfe04b63cc349fe6cc7f5aaf92cf21a1a11dd57225298aa716f9f4ecda4",
            6_547_072,
        )],
    ),
];

const H2: &[Release] = &[
    (
        "2.5.252",
        &[(
            "com/h2database/h2/2.5.252/h2-2.5.252.jar",
            "90b11dc413da82070ef15fc943282c1408e6ffd34fa68998335fea8603b5ff20",
            2_693_360,
        )],
    ),
    (
        "2.4.240",
        &[(
            "com/h2database/h2/2.4.240/h2-2.4.240.jar",
            "29b70e427cc1c40cdc376283adbb0cc62853073797bb5fe5761f81fe73d57ce0",
            2_685_418,
        )],
    ),
    (
        "2.3.232",
        &[(
            "com/h2database/h2/2.3.232/h2-2.3.232.jar",
            "8dae62d22db8982c3dcb3826edb9c727c5d302063a67eef7d63d82de401f07d3",
            2_651_157,
        )],
    ),
];

const HSQLDB: &[Release] = &[
    (
        "2.7.4",
        &[(
            "org/hsqldb/hsqldb/2.7.4/hsqldb-2.7.4.jar",
            "5fab2bb4384ac06b762638c8fa2740c944b8d080e4796c0c6c2af8b90dd4e5ad",
            1_662_185,
        )],
    ),
    (
        "2.7.3",
        &[(
            "org/hsqldb/hsqldb/2.7.3/hsqldb-2.7.3.jar",
            "6f2f77eedbe75cfbe26bf30d73b13de0cc57fb7cdb27a92ed8c1a012f0e2363a",
            1_657_682,
        )],
    ),
    (
        "2.7.2",
        &[(
            "org/hsqldb/hsqldb/2.7.2/hsqldb-2.7.2.jar",
            "aa455133e664f6a7e6f30cd0cd4f8ad83dfbd94eb717c438548e446784614a92",
            1_644_340,
        )],
    ),
];

const FIREBIRD: &[Release] = &[
    (
        "6.0.7",
        &[
            (
                "org/firebirdsql/jdbc/jaybird/6.0.7/jaybird-6.0.7.jar",
                "c0186747e4d56c6083095b722a6da2ae513735c9e6f53c17e48a42f4c4a12158",
                1_603_885,
            ),
            (
                "org/jspecify/jspecify/1.0.0/jspecify-1.0.0.jar",
                "1fad6e6be7557781e4d33729d49ae1cdc8fdda6fe477bb0cc68ce351eafdfbab",
                3_819,
            ),
        ],
    ),
    (
        "5.0.13.java11",
        &[(
            "org/firebirdsql/jdbc/jaybird/5.0.13.java11/jaybird-5.0.13.java11.jar",
            "de0d6c0a2dcd05a5622f13d1074b9a2b6de7df827bf245e799bba6b3f8774d4c",
            1_583_226,
        )],
    ),
    (
        "5.0.12.java11",
        &[(
            "org/firebirdsql/jdbc/jaybird/5.0.12.java11/jaybird-5.0.12.java11.jar",
            "3844dbbc59373b12a4943aa615a8bbec8d8a642685f6d01d6843bca0e1f76472",
            1_577_361,
        )],
    ),
];

const DERBY: &[Release] = &[
    (
        "10.17.1.0",
        &[
            (
                "org/apache/derby/derbyclient/10.17.1.0/derbyclient-10.17.1.0.jar",
                "8f3e7a1f2eec23faf8c1c0fb21767b37c4d60d32223362a1f82a740b39b3726a",
                600_922,
            ),
            (
                "org/apache/derby/derbyshared/10.17.1.0/derbyshared-10.17.1.0.jar",
                "fb68bab30785375c1f8cb5e9583b349750f8904d6f7ee15ce8e1c30b6be30ef4",
                88_762,
            ),
        ],
    ),
    (
        "10.16.1.1",
        &[
            (
                "org/apache/derby/derbyclient/10.16.1.1/derbyclient-10.16.1.1.jar",
                "b7243184b1df01b946eaf89eb904ed724d1f7493eb7467e85f4c6ffb4a09142b",
                603_730,
            ),
            (
                "org/apache/derby/derbyshared/10.16.1.1/derbyshared-10.16.1.1.jar",
                "27d4be683a45f6c15940167277ce39bb7e26b9f6dc0bc05efbcf813cac5d2b8f",
                88_705,
            ),
        ],
    ),
    (
        "10.15.2.0",
        &[
            (
                "org/apache/derby/derbyclient/10.15.2.0/derbyclient-10.15.2.0.jar",
                "f70d0bffda12dbffc3a348b82828ea9ccb329ca02d4c2cd86399362fcbc0c1f1",
                601_103,
            ),
            (
                "org/apache/derby/derbyshared/10.15.2.0/derbyshared-10.15.2.0.jar",
                "55365ab97e698080c6ccec65dbf7b8c63e4b4b77ad08f794d11458b1f2ea272c",
                93_688,
            ),
        ],
    ),
];

const HANA: &[Release] = &[
    (
        "2.30.12",
        &[(
            "com/sap/cloud/db/jdbc/ngdbc/2.30.12/ngdbc-2.30.12.jar",
            "9367dbb9be1a9aeeb019ed5c0205f300a3430a9cade10877f5f79d15cfe63532",
            1_588_079,
        )],
    ),
    (
        "2.29.11",
        &[(
            "com/sap/cloud/db/jdbc/ngdbc/2.29.11/ngdbc-2.29.11.jar",
            "225a89e3e3c70833628d44e5bbb42f5dbadefd1b9b2861687deb9b811db9568e",
            1_587_490,
        )],
    ),
    (
        "2.28.8",
        &[(
            "com/sap/cloud/db/jdbc/ngdbc/2.28.8/ngdbc-2.28.8.jar",
            "e3f74c7ae97187fdf7e561afaaf29d79f15ef6ec8bed2210e81d41728234b234",
            1_550_511,
        )],
    ),
];

const EXASOL: &[Release] = &[
    (
        "26.2.10",
        &[(
            "com/exasol/exasol-jdbc/26.2.10/exasol-jdbc-26.2.10.jar",
            "91b11bd846b0c447a1e9fed86541bc2c1b4c9272f9a2d0f169e9ff0bca222d19",
            9_525_280,
        )],
    ),
    (
        "25.2.6",
        &[(
            "com/exasol/exasol-jdbc/25.2.6/exasol-jdbc-25.2.6.jar",
            "5fa530718a7765ab8bd4ed4c238a489e987cd67e99589f78b8f1d3623b848faf",
            10_485_814,
        )],
    ),
    (
        "24.2.1",
        &[(
            "com/exasol/exasol-jdbc/24.2.1/exasol-jdbc-24.2.1.jar",
            "fe44245cb3defd02c810facbc35d73d81f74b0f191de076b218c87b22816ebf8",
            10_548_203,
        )],
    ),
];

const VERTICA: &[Release] = &[
    (
        "25.3.0-0",
        &[(
            "com/vertica/jdbc/vertica-jdbc/25.3.0-0/vertica-jdbc-25.3.0-0.jar",
            "f43c4ed61a9639d95214e00c888fd21c4ea283d60ca2ce06c6855a774ff5e583",
            1_850_793,
        )],
    ),
    (
        "25.2.0-0",
        &[(
            "com/vertica/jdbc/vertica-jdbc/25.2.0-0/vertica-jdbc-25.2.0-0.jar",
            "f2c95585468b3783c2eba8056796f8a2a975cdeb0ee598f378d5022cf6e078a7",
            1_850_793,
        )],
    ),
    (
        "24.4.0-0",
        &[(
            "com/vertica/jdbc/vertica-jdbc/24.4.0-0/vertica-jdbc-24.4.0-0.jar",
            "cc720c960d2280643b5c1d463a15e05a97fe9f456b30d1dfb5f16405ecbb0637",
            1_494_162,
        )],
    ),
];

const CATALOG: &[Catalog] = &[
    Catalog {
        name: "db2jcc",
        title: "IBM Data Server Driver for JDBC and SQLJ",
        license: "IBM International Program License Agreement",
        license_url: "https://www.ibm.com/support/customer/csol/terms/?ref=L-XFLK-S5N8U5-01-03-2026-zz-en",
        open_source: false,
        vendor_url: "https://repo1.maven.org/maven2/com/ibm/db2/jcc/",
        url_template: "jdbc:db2://{host}:{port}/{database}",
        default_port: Some(50000),
        releases: DB2JCC,
    },
    Catalog {
        name: "h2",
        title: "H2",
        license: "MPL 2.0 / EPL 1.0",
        license_url: "https://www.mozilla.org/en-US/MPL/2.0/",
        open_source: true,
        vendor_url: "https://h2database.com/",
        url_template: "jdbc:h2:tcp://{host}:{port}/{database}",
        default_port: Some(9092),
        releases: H2,
    },
    Catalog {
        name: "hsqldb",
        title: "HSQLDB",
        license: "BSD",
        license_url: "http://hsqldb.org/web/hsqlLicense.html",
        open_source: true,
        vendor_url: "https://hsqldb.org/",
        url_template: "jdbc:hsqldb:hsql://{host}:{port}/{database}",
        default_port: Some(9001),
        releases: HSQLDB,
    },
    Catalog {
        name: "firebird",
        title: "Firebird (Jaybird)",
        license: "LGPL",
        license_url: "https://www.gnu.org/copyleft/lesser.html",
        open_source: true,
        vendor_url: "https://firebirdsql.org/en/jdbc-driver/",
        url_template: "jdbc:firebirdsql://{host}:{port}/{database}",
        default_port: Some(3050),
        releases: FIREBIRD,
    },
    Catalog {
        name: "derby",
        title: "Apache Derby (network client)",
        license: "Apache License 2.0",
        license_url: "https://www.apache.org/licenses/LICENSE-2.0",
        open_source: true,
        vendor_url: "https://db.apache.org/derby/",
        url_template: "jdbc:derby://{host}:{port}/{database}",
        default_port: Some(1527),
        releases: DERBY,
    },
    Catalog {
        name: "hana",
        title: "SAP HANA",
        license: "SAP Developer License Agreement",
        license_url: "https://tools.hana.ondemand.com/developer-license-3_2.txt",
        open_source: false,
        vendor_url: "https://help.sap.com/docs/SAP_HANA_CLIENT",
        url_template: "jdbc:sap://{host}:{port}/?databaseName={database}",
        default_port: Some(30015),
        releases: HANA,
    },
    Catalog {
        name: "exasol",
        title: "Exasol",
        license: "EXAClient License",
        license_url: "https://repo1.maven.org/maven2/com/exasol/exasol-jdbc/26.2.10/exasol-jdbc-26.2.10-license.txt",
        open_source: false,
        vendor_url: "https://docs.exasol.com/db/latest/connect_exasol/drivers/jdbc.htm",
        url_template: "jdbc:exa:{host}:{port};schema={database}",
        default_port: Some(8563),
        releases: EXASOL,
    },
    Catalog {
        name: "vertica",
        title: "Vertica",
        license: "Vertica Client License Agreement",
        license_url: "https://www.vertica.com/wp-content/uploads/2024/02/Vertica-Client-License-Agreement_Feb2023.pdf",
        open_source: false,
        vendor_url: "https://docs.vertica.com/latest/en/connecting-to/client-libraries/accessing/",
        url_template: "jdbc:vertica://{host}:{port}/{database}",
        default_port: Some(5433),
        releases: VERTICA,
    },
];

fn release(c: &Catalog, (version, files): &Release) -> JdbcDriver {
    JdbcDriver {
        name: c.name.to_owned(),
        title: c.title.to_owned(),
        version: (*version).to_owned(),
        license: c.license.to_owned(),
        license_url: c.license_url.to_owned(),
        open_source: c.open_source,
        vendor_url: c.vendor_url.to_owned(),
        url_template: c.url_template.to_owned(),
        default_port: c.default_port,
        files: files
            .iter()
            .map(|(path, sha256, size)| JdbcFile {
                file: path.rsplit('/').next().unwrap_or(path).to_owned(),
                url: format!("{MAVEN}{path}"),
                sha256: (*sha256).to_owned(),
                size: *size,
            })
            .collect(),
    }
}

/// The versions of JDBC driver `name` the app can download, newest first.
#[must_use]
pub fn jdbc_versions(name: &str) -> Vec<JdbcDriver> {
    CATALOG
        .iter()
        .filter(|c| c.name == name)
        .flat_map(|c| c.releases.iter().map(|r| release(c, r)))
        .collect()
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

/// The jars of `driver`, when all of them are downloaded.
#[must_use]
pub fn installed_jdbc(data_dir: &Path, driver: &JdbcDriver) -> Option<Vec<PathBuf>> {
    let dir = jdbc_dir(data_dir);
    driver
        .files
        .iter()
        .map(|f| Some(dir.join(&f.file)).filter(|p| p.is_file()))
        .collect()
}

/// Downloads the jars of `driver` and keeps them. `progress` gets the bytes received of all the
/// files together, and their total.
pub async fn install_jdbc(
    data_dir: &Path,
    driver: &JdbcDriver,
    progress: impl Fn(u64, Option<u64>),
    cancel: &CancellationToken,
) -> Result<Vec<PathBuf>, DriverError> {
    let dir = jdbc_dir(data_dir);
    let total = driver.size();
    let mut before = 0;
    let mut paths = Vec::new();
    for file in &driver.files {
        let http = client(file.url.starts_with("https:"))?;
        let part = |done, _| progress(before + done, Some(total));
        let bytes = download(&http, &file.url, part, cancel).await?;
        check(&bytes, &file.sha256)?;
        std::fs::create_dir_all(&dir).map_err(io_error(&dir))?;
        let path = dir.join(&file.file);
        let tmp = dir.join(format!("{}.{}.part", file.file, std::process::id()));
        std::fs::write(&tmp, &bytes).map_err(io_error(&tmp))?;
        let _ = std::fs::remove_file(&path);
        std::fs::rename(&tmp, &path).map_err(io_error(&path))?;
        before += file.size;
        paths.push(path);
    }
    Ok(paths)
}

/// Deletes the downloaded jars of `driver`, those there are.
pub fn remove_jdbc(data_dir: &Path, driver: &JdbcDriver) -> Result<(), DriverError> {
    for file in &driver.files {
        let path = jdbc_dir(data_dir).join(&file.file);
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(io_error(&path)(e));
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_driver_has_files_checksums_and_a_url_template() {
        assert_eq!(JDBC_DRIVERS.len(), CATALOG.len());
        for name in JDBC_DRIVERS {
            let versions = jdbc_versions(name);
            assert!(versions.len() >= 2, "{name}");
            for d in &versions {
                assert!(!d.files.is_empty(), "{name} {}", d.version);
                assert!(d.url_template.starts_with("jdbc:"), "{name}");
                assert!(d.url_template.contains("{host}") && d.url_template.contains("{database}"));
                assert!(d.default_port.is_some(), "{name}");
                assert!(d.license_url.starts_with("http"), "{name}");
                for f in &d.files {
                    assert!(f.url.ends_with(&f.file), "{}", f.url);
                    assert!(f.url.starts_with("https://repo1.maven.org/maven2/"));
                    assert_eq!(f.sha256.len(), 64, "{}", f.file);
                    assert!(
                        f.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                        "{}",
                        f.file
                    );
                    assert!(f.size > 0, "{}", f.file);
                }
            }
            assert_eq!(
                jdbc_driver(name, None).unwrap().version,
                versions[0].version
            );
        }
        assert!(jdbc_versions("nothing").is_empty());
    }

    #[test]
    fn a_driver_with_dependencies_lists_every_jar() {
        let derby = jdbc_driver("derby", Some("10.16.1.1")).unwrap();
        let names: Vec<&str> = derby.files.iter().map(|f| f.file.as_str()).collect();
        assert_eq!(
            names,
            ["derbyclient-10.16.1.1.jar", "derbyshared-10.16.1.1.jar"]
        );
        assert_eq!(
            derby.size(),
            derby.files.iter().map(|f| f.size).sum::<u64>()
        );
        assert_eq!(jdbc_driver("firebird", None).unwrap().files.len(), 2);
    }

    #[test]
    fn a_version_is_found_by_its_number() {
        assert_eq!(
            jdbc_driver("db2jcc", Some("11.5.9.0")).unwrap().files[0].file,
            "jcc-11.5.9.0.jar"
        );
        assert_eq!(jdbc_driver("db2jcc", None).unwrap().version, "12.1.5.0");
        assert_eq!(jdbc_versions("db2jcc").len(), 6);
        assert!(jdbc_driver("db2jcc", Some("1.0")).is_none());
        assert!(!jdbc_driver("db2jcc", None).unwrap().open_source);
        assert!(jdbc_driver("h2", None).unwrap().open_source);
    }

    #[test]
    fn jars_that_are_there_are_installed_and_can_be_removed() {
        let dir = std::env::temp_dir().join(format!("platypus-jdbc-{}", std::process::id()));
        let driver = jdbc_driver("derby", Some("10.15.2.0")).unwrap();
        assert!(installed_jdbc(&dir, &driver).is_none());
        std::fs::create_dir_all(jdbc_dir(&dir)).unwrap();
        // One jar of two is not an installed driver.
        std::fs::write(jdbc_dir(&dir).join(&driver.files[0].file), b"PK").unwrap();
        assert!(installed_jdbc(&dir, &driver).is_none());
        std::fs::write(jdbc_dir(&dir).join(&driver.files[1].file), b"PK").unwrap();
        assert_eq!(installed_jdbc(&dir, &driver).unwrap().len(), 2);
        remove_jdbc(&dir, &driver).unwrap();
        assert!(installed_jdbc(&dir, &driver).is_none());
        remove_jdbc(&dir, &driver).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Downloads every file of the catalog and checks it against its checksum, which proves
    /// the URLs and the SHA-256 values. Needs the network (about 150 MB):
    /// `cargo test -p platypus-drivers -- --ignored every_catalog`.
    #[tokio::test]
    #[ignore = "downloads every catalog file from Maven Central"]
    async fn every_catalog_entry_installs() {
        let dir = std::env::temp_dir().join(format!("platypus-jdbc-dl-{}", std::process::id()));
        for name in JDBC_DRIVERS {
            for driver in jdbc_versions(name) {
                let paths = install_jdbc(&dir, &driver, |_, _| {}, &CancellationToken::new())
                    .await
                    .unwrap_or_else(|e| panic!("{name} {}: {e}", driver.version));
                assert_eq!(paths.len(), driver.files.len());
                assert!(installed_jdbc(&dir, &driver).is_some());
                remove_jdbc(&dir, &driver).unwrap();
            }
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
