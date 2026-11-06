//! The catalog against a local server: each check that stands between an index and an install, with
//! the refusal it gives, and the example of `examples/plugins/catalog`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use blake2::{Blake2b512, Digest as _};
use platypus_plugins::catalog::{CatalogLimits, EntryStatus};
use platypus_plugins::{
    CatalogRequest, IndexEntry, PluginError, Plugins, Prepared, SignatureState, TrustedKey,
};
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zip::write::SimpleFileOptions;

// A server for the tests.

/// An answer: its status, headers and body.
type Route = (u16, Vec<(String, String)>, Vec<u8>);

#[derive(Clone, Default)]
struct Server {
    routes: Arc<Mutex<HashMap<String, Route>>>,
    requests: Arc<AtomicUsize>,
    port: u16,
}

impl Server {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = Self {
            port: listener.local_addr().unwrap().port(),
            ..Self::default()
        };
        let shared = server.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let shared = shared.clone();
                tokio::spawn(async move {
                    let mut buffer = vec![0; 8192];
                    let n = socket.read(&mut buffer).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buffer[..n]).into_owned();
                    let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
                    shared.requests.fetch_add(1, Ordering::SeqCst);
                    let (status, headers, body) = shared
                        .routes
                        .lock()
                        .unwrap()
                        .get(&path)
                        .cloned()
                        .unwrap_or((404, Vec::new(), b"no".to_vec()));
                    let mut answer = format!(
                        "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n",
                        body.len()
                    );
                    for (name, value) in headers {
                        let _ = write!(answer, "{name}: {value}\r\n");
                    }
                    answer.push_str("\r\n");
                    let _ = socket.write_all(answer.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        server
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn serve(&self, path: &str, body: &[u8]) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.to_owned(), (200, Vec::new(), body.to_vec()));
    }

    fn redirect(&self, path: &str, to: &str) {
        self.routes.lock().unwrap().insert(
            path.to_owned(),
            (
                302,
                vec![("location".to_owned(), to.to_owned())],
                Vec::new(),
            ),
        );
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

// Signing, as `minisign -S` does: a pre-hashed Ed25519 signature and a signed comment.

struct Signer {
    pair: Ed25519KeyPair,
    id: [u8; 8],
}

impl Signer {
    fn new(seed: u8) -> Self {
        Self {
            pair: Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap(),
            id: [seed; 8],
        }
    }

    fn trusted(&self, name: &str) -> TrustedKey {
        let mut bytes = b"Ed".to_vec();
        bytes.extend_from_slice(&self.id);
        bytes.extend_from_slice(self.pair.public_key().as_ref());
        TrustedKey::parse(name, &STANDARD.encode(bytes)).unwrap()
    }

    fn sign(&self, data: &[u8]) -> String {
        let signature = self.pair.sign(&Blake2b512::digest(data));
        let mut line = b"ED".to_vec();
        line.extend_from_slice(&self.id);
        line.extend_from_slice(signature.as_ref());
        let comment = "timestamp:1 file:package.zip";
        let mut signed = signature.as_ref().to_vec();
        signed.extend_from_slice(comment.as_bytes());
        let global = self.pair.sign(&signed);
        format!(
            "untrusted comment: signed for the tests\n{}\ntrusted comment: {comment}\n{}\n",
            STANDARD.encode(line),
            STANDARD.encode(global.as_ref())
        )
    }
}

// Packages.

fn theme_manifest(id: &str, version: &str) -> String {
    format!(
        "id = \"{id}\"\nname = \"Sea\"\nversion = \"{version}\"\napi = 2\nkinds = [\"theme\"]\n[theme]\nname = \"Sea\"\ndark = {{ bg = \"#0b1d26\" }}\n"
    )
}

fn zipped(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn theme_zip(id: &str, version: &str) -> Vec<u8> {
    zipped(&[("plugin.toml", theme_manifest(id, version).as_bytes())])
}

fn sha256(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

/// A catalog on a local server: its plugins, and what asks for them with a trusted key.
struct Fixture {
    server: Server,
    plugins: Plugins,
    signer: Signer,
    data: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        Self {
            server: Server::start().await,
            plugins: Plugins::new(data.path()),
            signer: Signer::new(7),
            data,
        }
    }

    /// Serves `bytes` as a package and gives the entry that lists it, signed.
    fn publish(&self, id: &str, version: &str, bytes: &[u8]) -> IndexEntry {
        let path = format!("/{id}-{version}.zip");
        self.server.serve(&path, bytes);
        IndexEntry {
            id: id.to_owned(),
            name: "Sea".to_owned(),
            version: version.to_owned(),
            author: None,
            description: None,
            url: self.server.url(&path),
            sha256: sha256(bytes),
            signature: Some(self.signer.sign(bytes)),
        }
    }

    fn request(&self, entry: IndexEntry) -> CatalogRequest {
        CatalogRequest {
            entry,
            index_url: self.server.url("/index.json"),
            keys: vec![self.signer.trusted("Jane")],
            allow_unsigned: false,
            allow_downgrade: false,
        }
    }

    async fn prepare(&self, request: CatalogRequest) -> Result<Prepared, PluginError> {
        self.plugins
            .catalog_prepare(request, CatalogLimits::default())
            .await
    }

    /// A download left in the data folder, of any plugin.
    fn downloads(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.data.path().join("plugins/.downloads"))
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    }

    async fn installs(&self, entry: IndexEntry) {
        let prepared = self.prepare(self.request(entry)).await.unwrap();
        self.plugins
            .catalog_install(&prepared.ticket, &prepared.inspected.hash)
            .await
            .unwrap();
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used on the error of a call, in place"
)]
fn message(e: PluginError) -> String {
    e.to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn no_address_contacts_nothing() {
    let f = Fixture::new().await;
    let view = f
        .plugins
        .catalog_fetch("", CatalogLimits::default())
        .await
        .unwrap();
    assert_eq!(view.items.len(), 0);
    assert_eq!(f.server.requests(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_an_index_against_what_is_installed() {
    let f = Fixture::new().await;
    let installed = f.publish("sea", "1.0.0", &theme_zip("sea", "1.0.0"));
    f.installs(installed.clone()).await;
    let other = f.publish("other", "1.0.0", &theme_zip("other", "1.0.0"));
    f.installs(other).await;
    let older = f.publish("old", "2.0.0", &theme_zip("old", "2.0.0"));
    f.installs(older).await;

    let mut newer = installed.clone();
    newer.version = "1.1.0".to_owned();
    let mut fresh = installed.clone();
    fresh.id = "fresh".to_owned();
    let mut same = installed.clone();
    same.id = "other".to_owned();
    let mut behind = installed.clone();
    behind.id = "old".to_owned();
    let index = json!({ "format": 1, "plugins": [newer, fresh, same, behind, { "id": "broken" }] });
    f.server
        .serve("/index.json", &serde_json::to_vec(&index).unwrap());

    let view = f
        .plugins
        .catalog_fetch(&f.server.url("/index.json"), CatalogLimits::default())
        .await
        .unwrap();
    let got: Vec<_> = view
        .items
        .iter()
        .map(|item| {
            (
                item.entry.id.as_str(),
                item.status,
                item.installed.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("sea", EntryStatus::Update, Some("1.0.0")),
            ("fresh", EntryStatus::New, None),
            ("other", EntryStatus::Current, Some("1.0.0")),
            ("old", EntryStatus::Older, Some("2.0.0")),
        ]
    );
    assert_eq!(view.problems.len(), 1, "{:?}", view.problems);
    assert_eq!(view.problems[0].id.as_deref(), Some("broken"));
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_an_index_it_should_not_read() {
    let f = Fixture::new().await;
    let limits = CatalogLimits {
        index: 64,
        ..CatalogLimits::default()
    };
    // Not HTTPS, before anything is sent.
    let e = f
        .plugins
        .catalog_fetch("http://example.com/index.json", limits)
        .await;
    assert!(message(e.unwrap_err()).contains("not HTTPS"));

    // Larger than the limit, by its length.
    f.server.serve("/big.json", &[b' '; 65]);
    let e = f
        .plugins
        .catalog_fetch(&f.server.url("/big.json"), limits)
        .await;
    assert!(message(e.unwrap_err()).contains("larger than"));

    // An error from the server is said as such.
    let e = f
        .plugins
        .catalog_fetch(&f.server.url("/gone.json"), limits)
        .await;
    assert!(message(e.unwrap_err()).contains("404"));

    // A redirect goes through the same check.
    f.server
        .redirect("/moved.json", "http://example.invalid/index.json");
    let e = f
        .plugins
        .catalog_fetch(&f.server.url("/moved.json"), limits)
        .await;
    assert!(message(e.unwrap_err()).contains("not HTTPS"));

    // A redirect to where it may go is followed.
    f.server
        .serve("/index.json", br#"{"format":1,"plugins":[]}"#);
    f.server.redirect("/here.json", "/index.json");
    let view = f
        .plugins
        .catalog_fetch(&f.server.url("/here.json"), limits)
        .await
        .unwrap();
    assert_eq!(view.items.len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn installs_a_signed_package_after_its_review() {
    let f = Fixture::new().await;
    let entry = f.publish("sea", "1.0.0", &theme_zip("sea", "1.0.0"));
    let prepared = f.prepare(f.request(entry)).await.unwrap();
    assert_eq!(
        prepared.signature,
        SignatureState::Verified {
            key: "Jane".to_owned()
        }
    );
    assert_eq!(prepared.inspected.manifest.id, "sea");
    // Nothing is installed until the user answers.
    assert_eq!(f.plugins.list().unwrap().len(), 0);
    assert_eq!(f.downloads().len(), 1);

    // Installing takes the hash of the review.
    let e = f.plugins.catalog_install(&prepared.ticket, "other").await;
    assert!(message(e.unwrap_err()).contains("changed since"));
    assert_eq!(f.plugins.list().unwrap().len(), 0);
    let e = f
        .plugins
        .catalog_install(&prepared.ticket, &prepared.inspected.hash)
        .await;
    assert!(
        message(e.unwrap_err()).contains("gone"),
        "the ticket is spent"
    );

    let prepared = f
        .prepare(f.request(f.publish("sea", "1.0.0", &theme_zip("sea", "1.0.0"))))
        .await
        .unwrap();
    let installed = f
        .plugins
        .catalog_install(&prepared.ticket, &prepared.inspected.hash)
        .await
        .unwrap();
    assert_eq!(installed.manifest.version, "1.0.0");
    assert_eq!(f.plugins.list().unwrap().len(), 1);
    assert_eq!(f.downloads().len(), 0);
    assert!(!f.data.path().join("plugins/.downloads").exists());

    // A ticket that is not one reaches no file.
    let e = f.plugins.catalog_install("../../plugins.json", "x").await;
    assert!(message(e.unwrap_err()).contains("gone"));
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_a_review_down_leaves_nothing() {
    let f = Fixture::new().await;
    let entry = f.publish("sea", "1.0.0", &theme_zip("sea", "1.0.0"));
    let prepared = f.prepare(f.request(entry)).await.unwrap();
    f.plugins.catalog_discard(&prepared.ticket);
    assert!(!f.data.path().join("plugins/.downloads").exists());
    assert_eq!(f.plugins.list().unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_download_with_another_hash_is_refused() {
    let f = Fixture::new().await;
    let mut entry = f.publish("sea", "1.0.0", &theme_zip("sea", "1.0.0"));
    entry.sha256 = "00".repeat(32);
    let e = message(f.prepare(f.request(entry)).await.unwrap_err());
    assert!(e.contains("SHA-256"), "{e}");
    assert_eq!(f.downloads().len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_package_larger_than_the_limit_is_refused() {
    let f = Fixture::new().await;
    let entry = f.publish("sea", "1.0.0", &vec![0; 1024]);
    let limits = CatalogLimits {
        package: 1023,
        ..CatalogLimits::default()
    };
    let e = f.plugins.catalog_prepare(f.request(entry), limits).await;
    assert!(message(e.unwrap_err()).contains("larger than"));
    assert_eq!(f.downloads().len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_package_without_a_valid_signature_needs_its_own_confirmation() {
    let f = Fixture::new().await;
    let bytes = theme_zip("sea", "1.0.0");
    let good = f.publish("sea", "1.0.0", &bytes);
    let unsigned_error = async |request: CatalogRequest| match f.prepare(request).await {
        Err(PluginError::Unsigned(reason)) => reason,
        other => panic!("expected a refusal, got {other:?}"),
    };

    // No signature at all.
    let mut none = f.request(good.clone());
    none.entry.signature = None;
    assert!(unsigned_error(none.clone()).await.contains("no signature"));

    // No key trusted yet.
    let mut no_keys = f.request(good.clone());
    no_keys.keys.clear();
    assert!(unsigned_error(no_keys).await.contains("no key"));

    // Signed by a key the user does not trust.
    let mut other_key = f.request(good.clone());
    other_key.entry.signature = Some(Signer::new(9).sign(&bytes));
    assert!(unsigned_error(other_key).await.contains("key you trust"));

    // The trusted key's signature, of something else.
    let mut altered = f.request(good.clone());
    altered.entry.signature = Some(f.signer.sign(b"another package"));
    assert!(unsigned_error(altered).await.contains("altered"));

    // Not a signature.
    let mut unreadable = f.request(good.clone());
    unreadable.entry.signature = Some("hello".to_owned());
    assert!(unsigned_error(unreadable).await.contains("cannot be read"));

    assert_eq!(f.downloads().len(), 0);
    assert_eq!(f.plugins.list().unwrap().len(), 0);

    // The user's own confirmation lets it through, and the review says so.
    none.allow_unsigned = true;
    let prepared = f.prepare(none).await.unwrap();
    assert!(
        matches!(&prepared.signature, SignatureState::Unsigned { reason } if reason.contains("no signature"))
    );
    // It still has to be the package the index lists.
    let mut wrong = f.request(good);
    wrong.entry.signature = None;
    wrong.entry.sha256 = "00".repeat(32);
    wrong.allow_unsigned = true;
    assert!(message(f.prepare(wrong).await.unwrap_err()).contains("SHA-256"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_zip_with_paths_that_climb_out_is_refused() {
    let f = Fixture::new().await;
    let manifest = theme_manifest("sea", "1.0.0");
    for evil in [
        "../evil.txt",
        "a/../../evil.txt",
        "..\\evil.txt",
        "/etc/evil.txt",
        "C:\\evil.txt",
    ] {
        let bytes = zipped(&[("plugin.toml", manifest.as_bytes()), (evil, b"x")]);
        let entry = f.publish("sea", "1.0.0", &bytes);
        let e = message(f.prepare(f.request(entry)).await.unwrap_err());
        assert!(e.contains("climbs out"), "{evil}: {e}");
        assert_eq!(f.downloads().len(), 0);
    }
    assert_eq!(f.plugins.list().unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_package_that_is_not_the_entry_is_refused() {
    let f = Fixture::new().await;
    let entry = f.publish("sea", "1.0.0", &theme_zip("sand", "1.0.0"));
    let e = message(f.prepare(f.request(entry)).await.unwrap_err());
    assert!(e.contains("\"sand\"") && e.contains("\"sea\""), "{e}");

    let entry = f.publish("sea", "1.0.0", &theme_zip("sea", "1.0.1"));
    let e = message(f.prepare(f.request(entry)).await.unwrap_err());
    assert!(e.contains("1.0.1") && e.contains("1.0.0"), "{e}");
    assert_eq!(f.downloads().len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_downgrade_is_refused_unless_asked_for() {
    let f = Fixture::new().await;
    f.installs(f.publish("sea", "2.0.0", &theme_zip("sea", "2.0.0")))
        .await;
    let older = f.publish("sea", "1.5.0", &theme_zip("sea", "1.5.0"));
    let mut request = f.request(older);
    let e = f.prepare(request.clone()).await.unwrap_err();
    assert!(
        matches!(&e, PluginError::Downgrade { installed, offered } if installed == "2.0.0" && offered == "1.5.0"),
        "{e}"
    );
    assert_eq!(f.downloads().len(), 0);
    assert_eq!(f.plugins.list().unwrap()[0].manifest.version, "2.0.0");

    request.allow_downgrade = true;
    let prepared = f.prepare(request).await.unwrap();
    f.plugins
        .catalog_install(&prepared.ticket, &prepared.inspected.hash)
        .await
        .unwrap();
    assert_eq!(f.plugins.list().unwrap()[0].manifest.version, "1.5.0");
}

fn example(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins")
        .join(path)
}

/// The example of actions under `version`, with `permissions` for its clipboard one, zipped.
fn actions_zip(version: &str, permissions: &str) -> Vec<u8> {
    let module = std::fs::read(example("sql-tools/plugin.wasm")).unwrap();
    let manifest = std::fs::read_to_string(example("sql-tools/plugin.toml"))
        .unwrap()
        .replace("version = \"1.0.0\"", &format!("version = \"{version}\""))
        .replace("clipboard = true", permissions);
    zipped(&[
        ("plugin.toml", manifest.as_bytes()),
        ("plugin.wasm", &module),
    ])
}

#[tokio::test(flavor = "multi_thread")]
async fn an_update_asking_for_more_says_so_before_it_installs() {
    let f = Fixture::new().await;
    let entry = |version: &str, bytes: &[u8]| f.publish("sql-tools", version, bytes);
    let first = actions_zip("1.0.0", "clipboard = true");
    f.installs(entry("1.0.0", &first)).await;
    let approved = f.plugins.list().unwrap()[0].approved.clone();

    let more = actions_zip("1.1.0", "clipboard = true\nhttp = [\"api.example.com\"]");
    let prepared = f.prepare(f.request(entry("1.1.0", &more))).await.unwrap();
    assert_eq!(prepared.inspected.replaces.as_deref(), Some("1.0.0"));
    assert_eq!(prepared.inspected.added.http, ["api.example.com"]);
    // What is installed keeps what was approved until the user answers.
    assert_eq!(f.plugins.list().unwrap()[0].approved, approved);
    f.plugins
        .catalog_install(&prepared.ticket, &prepared.inspected.hash)
        .await
        .unwrap();
    assert_eq!(
        f.plugins.list().unwrap()[0].approved.http,
        ["api.example.com"]
    );
}

/// The example catalog: a theme package signed by the key of `trusted.pub`, listed by `index.json`.
/// Served as it is, the example installs; with any other key it is refused.
#[tokio::test(flavor = "multi_thread")]
async fn the_example_catalog_installs_and_its_denied_case_is_refused() {
    let f = Fixture::new().await;
    let index = std::fs::read(example("catalog/index.json")).unwrap();
    f.server.serve("/index.json", &index);
    f.server.serve(
        "/theme-sea-1.0.0.zip",
        &std::fs::read(example("catalog/theme-sea-1.0.0.zip")).unwrap(),
    );
    let view = f
        .plugins
        .catalog_fetch(&f.server.url("/index.json"), CatalogLimits::default())
        .await
        .unwrap();
    assert_eq!(view.problems, []);
    let entry = view.items[0].entry.clone();
    assert_eq!(entry.id, "theme-sea");

    // Denied: the key the user trusts is not the one that signed it.
    let denied = f.prepare(f.request(entry.clone())).await.unwrap_err();
    assert!(
        matches!(&denied, PluginError::Unsigned(reason) if reason.contains("key you trust")),
        "{denied}"
    );
    assert_eq!(f.plugins.list().unwrap().len(), 0);

    // Trusted: it installs, and the theme is there.
    let public = std::fs::read_to_string(example("catalog/trusted.pub")).unwrap();
    let mut request = f.request(entry);
    request.keys = vec![TrustedKey::parse("Example", &public).unwrap()];
    let prepared = f.prepare(request).await.unwrap();
    let installed = f
        .plugins
        .catalog_install(&prepared.ticket, &prepared.inspected.hash)
        .await
        .unwrap();
    assert!(installed.manifest.theme.is_some());
    assert!(!installed.manifest.needs_module());
}

/// The example theme installs as a folder, with no module; the same manifest with a value that tries
/// to reach a resource, or to leave its declaration, is refused by name.
#[tokio::test(flavor = "multi_thread")]
async fn the_example_theme_installs_and_its_denied_case_is_refused() {
    let f = Fixture::new().await;
    let folder = example("theme-sea");
    let reviewed = f.plugins.inspect(&folder).await.unwrap();
    assert!(reviewed.manifest.theme.is_some());
    assert!(reviewed.added.is_empty());
    f.plugins.install(&folder, &reviewed.hash).await.unwrap();
    let home = f.data.path().join("plugins/theme-sea/1.0.0");
    assert!(home.join("plugin.toml").exists());
    assert!(!home.join("plugin.wasm").exists());

    let manifest = std::fs::read_to_string(folder.join("plugin.toml")).unwrap();
    for (value, name) in [
        ("url(https://example.com/x.png)", "dark.bg"),
        ("#000000; background: url(x)", "dark.bg"),
        ("expression(alert(1))", "dark.bg"),
    ] {
        let denied = manifest.replace("#0b1d26", value);
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("plugin.toml"), denied).unwrap();
        let e = f
            .plugins
            .inspect(root.path())
            .await
            .unwrap_err()
            .to_string();
        assert!(
            e.contains(name) && e.contains("not a colour"),
            "{value}: {e}"
        );
    }
}
