//! Against a local HTTP server that serves an index and packages built in the test.

use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::{DriverError, Registry, installed, list, platform, remove};

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
