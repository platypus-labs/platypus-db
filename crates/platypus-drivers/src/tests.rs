//! Against a local HTTP server that serves an index and packages built in the test.

use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::{
    DriverError, Registry, activate, installed, list, platform, remove, remove_version, versions,
};

/// A `.tar.gz` like the registry's: the library, `MANIFEST`, `LICENSE`.
fn package(library: &str, body: &[u8]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    let manifest = format!(
        "name = \"Test Driver\"\nlicense = \"Apache-2.0\"\nversion = \"v1.2.0\"\n\n[Files]\ndriver = \"{library}\"\n"
    );
    for (name, data) in [
        ("MANIFEST", manifest.as_bytes()),
        ("LICENSE", b"license text".as_slice()),
        (library, body),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, name, data).unwrap();
    }
    let raw = tar.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&raw).unwrap();
    gz.finish().unwrap()
}

fn sha256(data: &[u8]) -> String {
    crate::install::hex(&Sha256::digest(data))
}

/// An index with `test` in two versions (only the newer for this platform has a hash) and
/// `closed`, under a license that is not free.
fn index(hash: &str) -> String {
    let p = platform();
    format!(
        "drivers:\n\
         - name: Test Driver\n  license: Apache-2.0\n  path: test\n  pkginfo:\n\
         \x20 - packages:\n    - platform: {p}\n      url: test/v1.10.0/pkg.tar.gz\n      hash: \n        sha256:{hash}\n    version: v1.10.0\n\
         \x20 - packages:\n    - platform: {p}\n      url: test/v1.9.0/pkg.tar.gz\n    version: v1.9.0\n\
         \x20 - packages:\n    - platform: other_os\n      url: test/v2.0.0/pkg.tar.gz\n    version: v2.0.0\n\
         - name: Closed Driver\n  license: LicenseRef-PBL\n  path: closed\n  pkginfo:\n\
         \x20 - packages:\n    - platform: {p}\n      url: closed/v1/pkg.tar.gz\n    version: v1.0.0\n"
    )
}

/// Serves `files` by path until the test ends; returns the base URL.
pub(crate) async fn serve(files: Vec<(&'static str, Vec<u8>)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let files = Arc::new(files);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let files = files.clone();
            tokio::spawn(async move {
                let mut buf = vec![0; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let found = files.iter().find(|(p, _)| *p == path);
                let (status, body) = match found {
                    Some((_, body)) => ("200 OK", body.clone()),
                    None => ("404 Not Found", Vec::new()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            });
        }
    });
    url
}

async fn registry_with(dir: &Path, hash_of: Option<&[u8]>) -> (Registry, Vec<u8>) {
    let pkg = package("libadbc_driver_test.dll", b"not really a library");
    let hash = sha256(hash_of.unwrap_or(&pkg));
    let url = serve(vec![
        ("/index.yaml", index(&hash).into_bytes()),
        ("/test/v1.10.0/pkg.tar.gz", pkg.clone()),
    ])
    .await;
    (Registry::with_url(&url, dir), pkg)
}

#[tokio::test]
async fn picks_latest_for_platform() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, pkg) = registry_with(dir.path(), None).await;
    let p = registry.package("test").await.unwrap();
    // v1.10.0 is newer than v1.9.0; v2.0.0 has nothing for this platform.
    assert_eq!(p.version, "v1.10.0");
    assert_eq!(p.license, "Apache-2.0");
    assert_eq!(p.title, "Test Driver");
    assert_eq!(p.sha256.as_deref(), Some(sha256(&pkg).as_str()));
    assert!(matches!(
        registry.package("missing").await,
        Err(DriverError::NoPackage { .. })
    ));
}

#[tokio::test]
async fn refuses_restricted_license() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = registry_with(dir.path(), None).await;
    assert!(matches!(
        registry.package("closed").await,
        Err(DriverError::License(_))
    ));
}

#[tokio::test]
async fn uses_cache_offline() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = registry_with(dir.path(), None).await;
    registry.package("test").await.unwrap();
    // Nothing listens there: the cached index answers.
    let offline = Registry::with_url("http://127.0.0.1:9", dir.path());
    assert_eq!(offline.package("test").await.unwrap().version, "v1.10.0");
    let empty = tempfile::tempdir().unwrap();
    let no_cache = Registry::with_url("http://127.0.0.1:9", empty.path());
    assert!(matches!(
        no_cache.package("test").await,
        Err(DriverError::Network(_))
    ));
}

#[tokio::test]
async fn installs_and_lists() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = registry_with(dir.path(), None).await;
    let p = registry.package("test").await.unwrap();
    let progress = std::sync::Mutex::new(Vec::new());
    let got = registry
        .install(
            &p,
            |done, total| progress.lock().unwrap().push((done, total)),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(got.name, "test");
    assert_eq!(got.version, "v1.10.0");
    assert!(got.library.ends_with("libadbc_driver_test.dll"));
    assert_eq!(
        std::fs::read(&got.library).unwrap(),
        b"not really a library"
    );
    assert!(!progress.lock().unwrap().is_empty());
    assert_eq!(installed(dir.path(), "test"), Some(got.clone()));
    assert_eq!(list(dir.path()), [got]);
    // Nothing left over from the download.
    let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("drivers"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[tokio::test]
async fn rejects_bad_checksum() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = registry_with(dir.path(), Some(b"something else")).await;
    let p = registry.package("test").await.unwrap();
    let err = registry
        .install(&p, |_, _| {}, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, DriverError::Checksum), "{err}");
    assert_eq!(installed(dir.path(), "test"), None);
}

#[tokio::test]
async fn cancel_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = registry_with(dir.path(), None).await;
    let p = registry.package("test").await.unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let err = registry.install(&p, |_, _| {}, &cancel).await.unwrap_err();
    assert!(matches!(err, DriverError::Cancelled), "{err}");
    assert_eq!(installed(dir.path(), "test"), None);
}

#[tokio::test]
async fn remove_driver() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = registry_with(dir.path(), None).await;
    let p = registry.package("test").await.unwrap();
    registry
        .install(&p, |_, _| {}, &CancellationToken::new())
        .await
        .unwrap();
    remove(dir.path(), "test").unwrap();
    assert_eq!(installed(dir.path(), "test"), None);
    assert_eq!(list(dir.path()), []);
}

#[tokio::test]
async fn refuses_a_package_without_checksum() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _) = registry_with(dir.path(), None).await;
    let mut p = registry.package("test").await.unwrap();
    p.sha256 = None;
    let err = registry
        .install(&p, |_, _| {}, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, DriverError::NoChecksum), "{err}");
    assert_eq!(installed(dir.path(), "test"), None);
}

#[test]
fn reads_the_hash_prefix_in_any_case() {
    use crate::registry::sha256_hex;
    assert_eq!(sha256_hex("sha256:ab12").as_deref(), Some("ab12"));
    assert_eq!(sha256_hex("  SHA256:AB12 ").as_deref(), Some("AB12"));
    assert_eq!(sha256_hex("Sha256:ab").as_deref(), Some("ab"));
    assert_eq!(sha256_hex("sha256:"), None);
    assert_eq!(sha256_hex("md5:ab12"), None);
    assert_eq!(sha256_hex("é"), None);
}

#[test]
fn orders_releases_after_their_prereleases() {
    use crate::registry::compare_versions;
    use std::cmp::Ordering::{Greater, Less};
    for (newer, older) in [
        ("v1.10.0", "v1.9.0"),
        ("1.0.0", "1.0.0-beta"),
        ("v1.0.0", "v1.0.0-rc.1"),
        ("1.0.0-rc.1", "1.0.0-beta.11"),
        ("1.0.0-beta.11", "1.0.0-beta.2"),
        ("1.0.0-beta", "1.0.0-alpha.1"),
        ("1.0.0-alpha.1", "1.0.0-alpha"),
        ("1.0.0-alpha.beta", "1.0.0-alpha.1"),
        ("1.0.1-beta", "1.0.0"),
    ] {
        assert_eq!(compare_versions(newer, older), Greater, "{newer} > {older}");
        assert_eq!(compare_versions(older, newer), Less, "{older} < {newer}");
    }
}

#[test]
fn refuses_driver_names_outside_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let adbc = dir.path().join("drivers").join("adbc");
    std::fs::create_dir_all(adbc.join("kept")).unwrap();
    let mut names = vec!["..", "../adbc", "a/b", "", ".", "/abs", "a/"];
    if cfg!(windows) {
        names.extend([r"a\b", "C:", r"..\adbc"]);
    }
    for name in names {
        assert!(
            matches!(remove(dir.path(), name), Err(DriverError::InvalidName(_))),
            "{name:?}"
        );
        assert_eq!(installed(dir.path(), name), None, "{name:?}");
    }
    assert!(adbc.join("kept").is_dir());
    assert!(remove(dir.path(), "missing").is_ok());
}

/// Two versions of `test` for this platform, each with its own package, and `other`. Returns
/// the registry and the two packages' bytes.
async fn registry_with_two_versions(dir: &Path) -> (Registry, Vec<u8>, Vec<u8>) {
    let new = package("libadbc_driver_test.dll", b"the newer library");
    let old = package("libadbc_driver_test.dll", b"the older library");
    let p = platform();
    let index = format!(
        "drivers:\n\
         - name: Test Driver\n  license: Apache-2.0\n  path: test\n  pkginfo:\n\
         \x20 - packages:\n    - platform: {p}\n      url: test/v1.9.0/pkg.tar.gz\n      hash: sha256:{}\n    version: v1.9.0\n\
         \x20 - packages:\n    - platform: {p}\n      url: test/v1.10.0/pkg.tar.gz\n      hash: sha256:{}\n    version: v1.10.0\n\
         - name: Other\n  license: MIT\n  path: other\n  pkginfo:\n\
         \x20 - packages:\n    - platform: {p}\n      url: other/v1/pkg.tar.gz\n      hash: sha256:{}\n    version: v1.0.0\n",
        sha256(&old),
        sha256(&new),
        sha256(&old),
    );
    let url = serve(vec![
        ("/index.yaml", index.into_bytes()),
        ("/test/v1.10.0/pkg.tar.gz", new.clone()),
        ("/test/v1.9.0/pkg.tar.gz", old.clone()),
        ("/other/v1/pkg.tar.gz", old.clone()),
    ])
    .await;
    (Registry::with_url(&url, dir), new, old)
}

/// Installs `version` of `test` from the registry.
async fn install_version(registry: &Registry, version: &str) -> crate::Installed {
    let packages = registry.packages("test").await.unwrap();
    let package = packages.iter().find(|p| p.version == version).unwrap();
    registry
        .install(package, |_, _| {}, &CancellationToken::new())
        .await
        .unwrap()
}

#[tokio::test]
async fn lists_every_version_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _, _) = registry_with_two_versions(dir.path()).await;
    let all = registry.packages("test").await.unwrap();
    let versions: Vec<_> = all.iter().map(|p| p.version.as_str()).collect();
    assert_eq!(versions, ["v1.10.0", "v1.9.0"]);
    assert!(all.iter().all(|p| p.sha256.is_some()));
    assert!(matches!(
        registry.packages("missing").await,
        Err(DriverError::NoPackage { .. })
    ));
}

#[tokio::test]
async fn versions_live_side_by_side_and_one_is_active() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _, _) = registry_with_two_versions(dir.path()).await;
    let old = install_version(&registry, "v1.9.0").await;
    assert_eq!(installed(dir.path(), "test"), Some(old.clone()));
    let new = install_version(&registry, "v1.10.0").await;
    assert_ne!(old.library, new.library);
    assert_eq!(installed(dir.path(), "test"), Some(new.clone()));
    let all = versions(dir.path(), "test");
    let seen: Vec<_> = all
        .iter()
        .map(|v| (v.installed.version.as_str(), v.active))
        .collect();
    assert_eq!(seen, [("v1.10.0", true), ("v1.9.0", false)]);
    // Going back keeps both on disk.
    activate(dir.path(), "test", "v1.9.0").unwrap();
    assert_eq!(installed(dir.path(), "test"), Some(old.clone()));
    assert_eq!(std::fs::read(&old.library).unwrap(), b"the older library");
    assert_eq!(std::fs::read(&new.library).unwrap(), b"the newer library");
    assert_eq!(list(dir.path()), [old]);
    // A version installed again is the one in use.
    install_version(&registry, "v1.10.0").await;
    assert_eq!(installed(dir.path(), "test"), Some(new));
}

#[tokio::test]
async fn activating_an_unknown_version_fails() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _, _) = registry_with_two_versions(dir.path()).await;
    let new = install_version(&registry, "v1.10.0").await;
    assert!(activate(dir.path(), "test", "v1.9.0").is_err());
    assert!(matches!(
        activate(dir.path(), "test", ".."),
        Err(DriverError::InvalidName(_))
    ));
    assert_eq!(installed(dir.path(), "test"), Some(new));
}

#[tokio::test]
async fn removing_a_version_keeps_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _, _) = registry_with_two_versions(dir.path()).await;
    let old = install_version(&registry, "v1.9.0").await;
    let new = install_version(&registry, "v1.10.0").await;
    remove_version(dir.path(), "test", "v1.9.0").unwrap();
    assert!(!old.library.exists());
    assert_eq!(installed(dir.path(), "test"), Some(new.clone()));
    // The active one removed: the one left takes over; with none left the driver is gone.
    let again = install_version(&registry, "v1.9.0").await;
    remove_version(dir.path(), "test", "v1.9.0").unwrap();
    assert_eq!(installed(dir.path(), "test"), Some(new.clone()));
    drop(again);
    remove_version(dir.path(), "test", "v1.10.0").unwrap();
    assert_eq!(installed(dir.path(), "test"), None);
    assert!(!new.library.exists());
    // A version that is not there, or a name outside the folder.
    remove_version(dir.path(), "test", "v9").unwrap();
    assert!(matches!(
        remove_version(dir.path(), "test", "../x"),
        Err(DriverError::InvalidName(_))
    ));
}

#[tokio::test]
async fn finds_updates_for_the_active_versions() {
    let dir = tempfile::tempdir().unwrap();
    let (registry, _, _) = registry_with_two_versions(dir.path()).await;
    let old = install_version(&registry, "v1.9.0").await;
    let found = registry.updates(&[old]).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "test");
    assert_eq!(found[0].installed, "v1.9.0");
    assert_eq!(found[0].package.version, "v1.10.0");
    // Installing it leaves the old one for rollback and ends the update.
    let new = install_version(&registry, &found[0].package.version).await;
    assert_eq!(registry.updates(&[new]).await.unwrap().len(), 0);
    assert_eq!(versions(dir.path(), "test").len(), 2);
    // A driver the registry does not know has nothing to update to.
    let unknown = crate::Installed {
        name: "gone".to_owned(),
        title: "Gone".to_owned(),
        version: "v1.0.0".to_owned(),
        license: "MIT".to_owned(),
        library: "x".into(),
    };
    assert_eq!(registry.updates(&[unknown]).await.unwrap().len(), 0);
}

#[test]
fn chooses_the_newest_package_only_when_it_is_newer() {
    let package = |version: &str| crate::Package {
        name: "test".to_owned(),
        title: "Test".to_owned(),
        version: version.to_owned(),
        license: "MIT".to_owned(),
        url: String::new(),
        sha256: None,
    };
    let all = [package("v1.9.0"), package("v1.10.0"), package("v1.2.0")];
    assert_eq!(
        crate::newer_than("v1.9.0", &all).unwrap().version,
        "v1.10.0"
    );
    assert_eq!(
        crate::newer_than("v1.2.0", &all).unwrap().version,
        "v1.10.0"
    );
    assert!(crate::newer_than("v1.10.0", &all).is_none());
    assert!(crate::newer_than("v2.0.0", &all).is_none());
    // A prerelease is older than its release.
    assert!(crate::newer_than("1.0.0", &[package("1.0.0-rc.1")]).is_none());
    assert!(crate::newer_than("1.0.0", &[]).is_none());
}

#[test]
fn asks_for_updates_at_most_once_a_day() {
    use crate::update_check_due;
    let day = 24 * 60 * 60;
    assert!(update_check_due(None, 1_000));
    assert!(!update_check_due(Some(1_000), 1_000 + day - 1));
    assert!(update_check_due(Some(1_000), 1_000 + day));
    // The clock went back: ask again rather than wait for it to catch up.
    assert!(update_check_due(Some(5_000), 1_000));
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(crate::last_update_check(dir.path()), None);
    crate::record_update_check(dir.path(), 1_234).unwrap();
    assert_eq!(crate::last_update_check(dir.path()), Some(1_234));
}

/// Writes a driver folder the way an install does, with its `driver.toml`.
fn write_driver(dir: &Path, version: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("lib.dll"), version).unwrap();
    std::fs::write(
        dir.join("driver.toml"),
        format!(
            "name = \"test\"\ntitle = \"Test\"\nversion = \"{version}\"\nlicense = \"MIT\"\nlibrary = \"lib.dll\"\n"
        ),
    )
    .unwrap();
}

#[test]
fn moves_a_driver_without_version_folders_without_downloading_it() {
    let dir = tempfile::tempdir().unwrap();
    let flat = dir.path().join("drivers").join("adbc").join("test");
    write_driver(&flat, "v1.0.0");
    assert_eq!(installed(dir.path(), "test"), None);
    crate::migrate_layout(dir.path());
    let got = installed(dir.path(), "test").unwrap();
    assert_eq!(got.version, "v1.0.0");
    assert_eq!(got.library, flat.join("v1.0.0").join("lib.dll"));
    assert_eq!(std::fs::read(&got.library).unwrap(), b"v1.0.0");
    assert!(!flat.join("driver.toml").exists());
    // Running it again changes nothing.
    crate::migrate_layout(dir.path());
    assert_eq!(installed(dir.path(), "test"), Some(got));
}

#[test]
fn a_chosen_version_that_vanished_falls_back_to_the_newest() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("drivers").join("adbc").join("test");
    for version in ["v1.9.0", "v1.10.0"] {
        write_driver(&root.join(version), version);
    }
    // No ACTIVE file, as before versions could be chosen: the newest.
    assert_eq!(installed(dir.path(), "test").unwrap().version, "v1.10.0");
    std::fs::write(root.join("ACTIVE"), "v1.9.0\n").unwrap();
    assert_eq!(installed(dir.path(), "test").unwrap().version, "v1.9.0");
    std::fs::write(root.join("ACTIVE"), "v0.1.0").unwrap();
    assert_eq!(installed(dir.path(), "test").unwrap().version, "v1.10.0");
}
