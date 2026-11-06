//! The documents of plugins' tabs and side panels, and what those documents may ask.
//!
//! A document is untrusted HTML. The app shows it in an `<iframe sandbox="allow-scripts">`, on an
//! opaque origin, loaded from a URI scheme of its own ([`FRAME_SCHEME`]); [`frame_csp`] is what
//! that scheme answers with, and it sandboxes the document again by itself, whoever opens it.
//! The app's own policy cannot reach it from a `srcdoc` or a `blob:`, which would inherit that
//! policy and its ban on inline scripts; a scheme is also the only way to give a document a
//! policy stricter in everything else (no network at all: `connect-src 'none'`).
//!
//! Files come only from the plugin's own folder, under a path [`is_package_path`] accepts, and
//! are copied there at install: the documents its manifest names and what is under `ui/`. Neither
//! the manifest nor the module can be served, and neither can any `.html` the manifest does not
//! name.
//!
//! The only way out of a frame is a message to the app, which turns the few it knows into a
//! [`FrameRequest`] for [`Plugins::frame_request`]. Each is checked here as well as in the page:
//! the permission it needs, its size and a rate per plugin. None of them can read a saved
//! connection, a password or a query's rows, and none runs SQL: `open_query` opens a tab with the
//! text, which runs only when the user runs it, through the app's checks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::extensions::is_package_path;
use crate::http::Http;
use crate::manifest::{Capability, MANIFEST_FILE, MODULE_FILE, Manifest};
use crate::package::{Plugins, blocking, read_file};
use crate::runtime::Limits;
use crate::{PluginError, settings};

/// The URI scheme of the documents: `http://<scheme>.localhost/<plugin>/<path>` on Windows and
/// Android, `<scheme>://localhost/<plugin>/<path>` elsewhere.
pub const FRAME_SCHEME: &str = "platypus-plugin";
/// The folder of a package whose files are copied for its documents to load.
const ASSETS_DIR: &str = "ui";
/// The largest file of those.
pub(crate) const MAX_FILE: u64 = 8 << 20;
/// How many of them a package may have.
const MAX_FILES: usize = 256;
/// All of them together.
const MAX_FILES_BYTES: u64 = 32 << 20;
/// The deepest folder walked under `ui/`.
const MAX_DEPTH: usize = 8;

/// The largest request of a frame, as JSON.
pub const MAX_REQUEST: usize = 1 << 20;
/// The largest answer to an HTTP request of a frame.
const MAX_HTTP_ANSWER: usize = 4 << 20;
/// The longest text a frame may show as a message, in characters.
const MAX_NOTICE: usize = 500;
/// The largest state a plugin's documents may keep.
const MAX_STATE: usize = 256 << 10;
/// How many requests a plugin's documents may have answered in a burst, and per second after it.
const RATE_BURST: f64 = 100.0;
const RATE_PER_SECOND: f64 = 50.0;

/// The folder under the plugins' where the documents' state is kept: with its dot, no plugin's id.
const STATE_DIR: &str = ".frame-state";

/// The files of a package besides its manifest and module, by path inside it.
pub(crate) type PackageFiles = BTreeMap<String, Vec<u8>>;

/// The script put at the start of each document: `window.platypus`, a thin layer over messages.
const BOOTSTRAP: &str = include_str!("frame_bootstrap.js");

/// What a file may be, by its extension; anything else is neither copied nor served.
fn mime_of(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        _ => return None,
    })
}

fn is_document(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".html")
}

/// What a package may hold in files: spent while they are read.
#[derive(Debug, Default)]
pub(crate) struct Budget {
    files: usize,
    bytes: u64,
}

impl Budget {
    pub(crate) fn spend(&mut self, path: &str, bytes: usize) -> Result<(), PluginError> {
        self.files += 1;
        self.bytes += bytes as u64;
        if self.files > MAX_FILES {
            return Err(PluginError::Invalid(format!(
                "the package has more than {MAX_FILES} files for its pages"
            )));
        }
        if self.bytes > MAX_FILES_BYTES {
            return Err(PluginError::Invalid(format!(
                "{path}: the files of the package's pages are larger than {} MB",
                MAX_FILES_BYTES >> 20
            )));
        }
        Ok(())
    }
}

/// Which of `names`, the paths found in a package, are its documents' files: the documents of the
/// manifest, all of them there, and what is under `ui/` of a kind that may be served. A name under
/// `ui/` that climbs out or hides is a broken package, not something to skip.
pub(crate) fn choose_files(
    manifest: &Manifest,
    names: &[String],
) -> Result<BTreeSet<String>, PluginError> {
    let mut chosen = BTreeSet::new();
    for document in manifest.documents() {
        if !names.iter().any(|n| n == document) {
            return Err(PluginError::Invalid(format!(
                "{document} is in {MANIFEST_FILE} and not in the package"
            )));
        }
        chosen.insert(document.to_owned());
    }
    if chosen.is_empty() {
        return Ok(chosen);
    }
    let assets = format!("{ASSETS_DIR}/");
    for name in names {
        let Some(rest) = name.strip_prefix(&assets) else {
            continue;
        };
        // The folder itself, in a .zip.
        if rest.is_empty() || rest.ends_with('/') {
            continue;
        }
        if !is_package_path(name) {
            return Err(PluginError::Invalid(format!(
                "{name:?} is not a path inside the package"
            )));
        }
        if mime_of(name).is_some() {
            chosen.insert(name.clone());
        }
    }
    Ok(chosen)
}

/// The paths of the files under `dir`, as `prefix/…`. A link is refused: it would point out of
/// the package.
fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) -> Result<(), PluginError> {
    if depth > MAX_DEPTH {
        return Err(PluginError::Invalid(format!(
            "{prefix} is too deep in the package"
        )));
    }
    for entry in std::fs::read_dir(dir).map_err(PluginError::io(dir))? {
        let entry = entry.map_err(PluginError::io(dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{prefix}/{name}");
        let kind = entry.file_type().map_err(PluginError::io(&entry.path()))?;
        if kind.is_symlink() {
            return Err(PluginError::Invalid(format!(
                "{path} is a link: a package's files stay inside it"
            )));
        }
        // A `.DS_Store` and the like are nobody's page.
        if name.starts_with('.') {
            continue;
        }
        if kind.is_dir() {
            walk(&entry.path(), &path, depth + 1, out)?;
        } else if kind.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

/// The files of the documents of a package kept in a folder.
pub(crate) fn read_folder_files(
    root: &Path,
    manifest: &Manifest,
) -> Result<PackageFiles, PluginError> {
    if manifest.documents().next().is_none() {
        return Ok(PackageFiles::new());
    }
    let mut names = Vec::new();
    for document in manifest.documents() {
        let path = root.join(document);
        // A document that is a link is one outside the package.
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => names.push(document.to_owned()),
            Ok(_) => {
                return Err(PluginError::Invalid(format!(
                    "{document} is not a plain file of the package"
                )));
            }
            Err(_) => {}
        }
    }
    let assets = root.join(ASSETS_DIR);
    if assets.is_dir() {
        if std::fs::symlink_metadata(&assets)
            .map_err(PluginError::io(&assets))?
            .is_symlink()
        {
            return Err(PluginError::Invalid(format!(
                "{ASSETS_DIR} is a link: a package's files stay inside it"
            )));
        }
        walk(&assets, ASSETS_DIR, 0, &mut names)?;
    }
    let mut files = PackageFiles::new();
    let mut budget = Budget::default();
    for path in choose_files(manifest, &names)? {
        let bytes = read_file(&root.join(&path), MAX_FILE)?;
        budget.spend(&path, bytes.len())?;
        files.insert(path, bytes);
    }
    Ok(files)
}

/// Writes the files of a package where it is being installed; their paths have been checked.
pub(crate) fn write_files(dir: &Path, files: &PackageFiles) -> Result<(), PluginError> {
    for (name, bytes) in files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(PluginError::io(parent))?;
        }
        std::fs::write(&path, bytes).map_err(PluginError::io(&path))?;
    }
    Ok(())
}

/// A file a frame loads, as the URI scheme answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameFile {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

/// The plugin and the path in the path of a request to [`FRAME_SCHEME`]: `/<plugin>/<path>`.
#[must_use]
pub fn frame_target(uri_path: &str) -> Option<(&str, &str)> {
    uri_path.strip_prefix('/')?.split_once('/')
}

/// The Content-Security-Policy a plugin's files are served with, where `base` is the scheme's
/// root for this system: scripts and styles inline or from its own folder, images and fonts from
/// there too, and nothing else. No network, no frames, no workers, no forms, no `<base>`; with
/// `sandbox` as well, the document has an opaque origin even if opened outside the app's frame.
#[must_use]
pub fn frame_csp(base: &str, plugin: &str) -> String {
    let own = format!("{base}/{plugin}/");
    format!(
        "sandbox allow-scripts; default-src 'none'; script-src 'unsafe-inline' {own}; \
         style-src 'unsafe-inline' {own}; img-src data: {own}; font-src data: {own}; \
         connect-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; \
         object-src 'none'; media-src 'none'; manifest-src 'none'; base-uri 'none'; \
         form-action 'none'; webrtc 'block'"
    )
}

/// `html` with the bootstrap as its first script: after a doctype, so that the document stays in
/// standards mode, and before anything of the plugin's.
fn with_bootstrap(html: &[u8]) -> Vec<u8> {
    let html = html.strip_prefix(b"\xef\xbb\xbf").unwrap_or(html);
    let script = format!("<script>{BOOTSTRAP}</script>");
    let start = html.iter().take_while(|b| b.is_ascii_whitespace()).count();
    let head = &html[start..];
    let doctype = head
        .get(..9)
        .is_some_and(|h| h.eq_ignore_ascii_case(b"<!doctype"))
        .then(|| head.iter().position(|b| *b == b'>'))
        .flatten()
        .map_or(0, |end| start + end + 1);
    let mut out = Vec::with_capacity(html.len() + script.len());
    out.extend_from_slice(&html[..doctype]);
    out.extend_from_slice(script.as_bytes());
    out.extend_from_slice(&html[doctype..]);
    out
}

/// One thing a plugin's document asks the app; all there is. An unknown name or field is an
/// error of the request, by name.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "name",
    content = "params",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FrameRequest {
    /// One of its own settings; never a secret.
    Setting { key: String },
    /// A message for the user, from the plugin.
    Notify { text: String },
    /// Text for the clipboard: needs `permissions.clipboard`.
    Clipboard { text: String },
    /// Opens a new SQL tab with this text, not run.
    OpenQuery { text: String },
    /// An HTTP request: needs the host in `permissions.http`.
    Http(Value),
    /// Runs an action of its own module, on what the document gives it.
    RunAction { action: String, context: Value },
    /// What its documents kept.
    StateGet {},
    /// Keeps a value for its documents.
    StateSet { value: Value },
}

impl FrameRequest {
    /// The request in `value`, refused when it is too large or not one the app knows.
    pub fn parse(value: Value) -> Result<Self, PluginError> {
        let size = serde_json::to_vec(&value).map_or(usize::MAX, |bytes| bytes.len());
        if size > MAX_REQUEST {
            return Err(PluginError::Denied(format!(
                "the request is larger than {} KB",
                MAX_REQUEST >> 10
            )));
        }
        serde_json::from_value(value)
            .map_err(|e| PluginError::Denied(format!("this request is not one the app knows: {e}")))
    }
}

/// How many requests a plugin's documents may make: a bucket refilled with time.
#[derive(Debug, Clone, Copy)]
struct Rate {
    tokens: f64,
    at: Instant,
}

impl Rate {
    fn full(now: Instant) -> Self {
        Self {
            tokens: RATE_BURST,
            at: now,
        }
    }

    fn take(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + elapsed * RATE_PER_SECOND).min(RATE_BURST);
        self.at = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// By the plugin's folder.
static RATES: Mutex<BTreeMap<PathBuf, Rate>> = Mutex::new(BTreeMap::new());

fn check_rate(plugin: PathBuf) -> Result<(), PluginError> {
    let now = Instant::now();
    let mut rates = RATES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let rate = rates.entry(plugin).or_insert_with(|| Rate::full(now));
    if rate.take(now) {
        Ok(())
    } else {
        Err(PluginError::Denied(
            "too many requests from this page".to_owned(),
        ))
    }
}

fn state_file(plugins_dir: &Path, id: &str) -> PathBuf {
    plugins_dir.join(STATE_DIR).join(format!("{id}.json"))
}

/// Forgets what a plugin's documents kept; nothing is lost but that.
pub(crate) fn forget_state(plugins_dir: &Path, id: &str) {
    let _ = std::fs::remove_file(state_file(plugins_dir, id));
}

impl Plugins {
    /// A file of the package of `id`, for one of its frames. Needs the `tabs` permission; a
    /// document must be one the manifest names, and anything else of a kind that may be served.
    /// Nothing outside the package's folder can be named, links included.
    pub fn frame_file(&self, id: &str, path: &str) -> Result<FrameFile, PluginError> {
        let installed = self.require(id, Capability::Tabs)?;
        let refused = || PluginError::Denied(format!("{path:?} is not a file of its pages"));
        if !is_package_path(path) || path == MANIFEST_FILE || path == MODULE_FILE {
            return Err(refused());
        }
        let mime = mime_of(path).ok_or_else(refused)?;
        if is_document(path) && !installed.manifest.documents().any(|d| d == path) {
            return Err(refused());
        }
        let root = self.module_path(&installed.manifest);
        let target = root.join(path);
        // Not a link, a folder or a device.
        let meta = std::fs::symlink_metadata(&target).map_err(|_| refused())?;
        if !meta.is_file() {
            return Err(refused());
        }
        let canonical = target.canonicalize().map_err(|_| refused())?;
        let canonical_root = root.canonicalize().map_err(PluginError::io(&root))?;
        if !canonical.starts_with(&canonical_root) {
            return Err(refused());
        }
        let bytes = read_file(&target, MAX_FILE)?;
        Ok(FrameFile {
            bytes: if is_document(path) {
                with_bootstrap(&bytes)
            } else {
                bytes
            },
            mime,
        })
    }

    /// The URL path of a document of `id`, once its permission is checked: what the page puts in
    /// the frame. A document the manifest does not name has none.
    pub fn frame_path(&self, id: &str, document: &str) -> Result<String, PluginError> {
        let installed = self.require(id, Capability::Tabs)?;
        if installed.manifest.documents().any(|d| d == document) {
            Ok(format!("/{id}/{document}"))
        } else {
            Err(PluginError::Denied(format!(
                "{document:?} is not a page of the plugin {id:?}"
            )))
        }
    }

    /// What a document of `id` asks: the answer is for the document, except a `notify`, a
    /// `clipboard` or an `open_query`, whose `text` the page then shows, copies or opens.
    pub async fn frame_request(
        &self,
        id: &str,
        request: FrameRequest,
    ) -> Result<Value, PluginError> {
        let installed = self.require(id, Capability::Tabs)?;
        check_rate(self.dir().join(id))?;
        match request {
            FrameRequest::Setting { key } => {
                let access = self
                    .grants(&installed)
                    .settings
                    .ok_or_else(|| PluginError::Denied("this plugin has no settings".to_owned()))?;
                settings::plain_value(&access, &key)
            }
            FrameRequest::Notify { text } => Ok(json!({ "text": clean_notice(&text) })),
            FrameRequest::Clipboard { text } => {
                if !installed.approved.clipboard {
                    return Err(PluginError::Denied(format!(
                        "the plugin {id:?} has no permission for the clipboard"
                    )));
                }
                Ok(json!({ "text": text }))
            }
            FrameRequest::OpenQuery { text } => Ok(json!({ "text": text })),
            FrameRequest::Http(params) => self.frame_http(&installed, params).await,
            FrameRequest::RunAction { action, context } => {
                let outcome = self.run_action(id, &action, context).await?;
                serde_json::to_value(outcome).map_err(|e| PluginError::Invalid(e.to_string()))
            }
            FrameRequest::StateGet {} => self.frame_state(id),
            FrameRequest::StateSet { value } => {
                self.set_frame_state(id, &value).map(|()| json!({}))
            }
        }
    }

    /// An HTTP request of a frame, by the app: the frame's own policy has no network at all.
    /// The same hosts as the module's, and the same client.
    async fn frame_http(
        &self,
        installed: &crate::package::Installed,
        params: Value,
    ) -> Result<Value, PluginError> {
        let grants = self.grants(installed);
        if grants.http.is_empty() {
            return Err(PluginError::Denied(format!(
                "the plugin {:?} has no permission for HTTP",
                grants.plugin
            )));
        }
        let limits = Limits {
            http_timeout: Duration::from_secs(30),
            ..self.limits()
        };
        blocking(move || {
            let http = Http::new(&grants, &limits)?;
            let raw = serde_json::to_vec(&params)
                .map_err(|e| PluginError::Invalid(format!("cannot read the request: {e}")))?;
            let deadline = Instant::now() + limits.http_timeout;
            let mut answer = http.request(&raw, deadline, MAX_HTTP_ANSWER);
            if let Some(ok) = answer.get_mut("ok").map(Value::take) {
                return Ok(ok);
            }
            let error = answer
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("the request failed");
            Err(if error.starts_with("not allowed") {
                PluginError::Denied(error.to_owned())
            } else {
                PluginError::Guest(error.to_owned())
            })
        })
        .await
    }

    fn frame_state(&self, id: &str) -> Result<Value, PluginError> {
        match std::fs::read(state_file(self.dir(), id)) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or(Value::Null)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
            Err(e) => Err(PluginError::io(&state_file(self.dir(), id))(e)),
        }
    }

    fn set_frame_state(&self, id: &str, value: &Value) -> Result<(), PluginError> {
        let bytes = serde_json::to_vec(value).map_err(|e| PluginError::Invalid(e.to_string()))?;
        if bytes.len() > MAX_STATE {
            return Err(PluginError::Denied(format!(
                "the state is larger than {} KB",
                MAX_STATE >> 10
            )));
        }
        let path = state_file(self.dir(), id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(PluginError::io(parent))?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes).map_err(PluginError::io(&tmp))?;
        std::fs::rename(&tmp, &path).map_err(PluginError::io(&path))
    }
}

/// A message of a document as the user reads it: one short line, plain text. The page draws it
/// as text, and says which plugin it comes from.
fn clean_notice(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_NOTICE)
        .collect();
    flat.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    fn manifest(entries: &[&str]) -> Manifest {
        let mut tabs = String::new();
        for (i, e) in entries.iter().enumerate() {
            writeln!(
                tabs,
                "[[tabs]]\nkind = \"k{i}\"\ntitle = \"K\"\nentry = \"{e}\""
            )
            .unwrap();
        }
        Manifest::parse(&format!(
            "id = \"p\"\nname = \"P\"\nversion = \"1\"\napi = 2\nkinds = [\"tabs\"]\n[permissions]\ntabs = true\n{tabs}"
        ))
        .unwrap()
    }

    #[test]
    fn chooses_the_documents_and_what_is_under_ui() {
        let names: Vec<String> = [
            "plugin.toml",
            "plugin.wasm",
            "ui/a.html",
            "ui/a.js",
            "ui/deep/b.css",
            "ui/notes.txt",
            "ui/",
            "src/lib.rs",
            "other.html",
            "root.html",
        ]
        .map(str::to_owned)
        .to_vec();
        let chosen = choose_files(&manifest(&["ui/a.html", "root.html"]), &names).unwrap();
        assert_eq!(
            chosen.into_iter().collect::<Vec<_>>(),
            ["root.html", "ui/a.html", "ui/a.js", "ui/deep/b.css"]
        );
    }

    #[test]
    fn a_document_must_be_in_the_package_and_a_path_may_not_climb_out() {
        let e = choose_files(&manifest(&["ui/a.html"]), &[]).unwrap_err();
        assert!(e.to_string().contains("not in the package"), "{e}");
        for bad in ["ui/../secret.js", "ui/.hidden.js", "ui/a\\b.js", "ui//a.js"] {
            let names = vec!["ui/a.html".to_owned(), bad.to_owned()];
            let e = choose_files(&manifest(&["ui/a.html"]), &names).unwrap_err();
            assert!(e.to_string().contains("inside the package"), "{bad}: {e}");
        }
    }

    #[test]
    fn a_package_without_documents_has_no_files() {
        let plain = Manifest::parse(
            "id = \"p\"\nname = \"P\"\nversion = \"1\"\napi = 1\nkinds = [\"actions\"]\n",
        )
        .unwrap();
        let names = vec!["ui/a.js".to_owned()];
        assert!(choose_files(&plain, &names).unwrap().is_empty());
    }

    #[test]
    fn only_what_a_page_loads_can_be_served() {
        for (path, kind) in [
            ("a.html", true),
            ("A.HTML", true),
            ("a.js", true),
            ("plugin.wasm", false),
            ("plugin.toml", false),
            ("a.exe", false),
            ("noextension", false),
        ] {
            assert_eq!(mime_of(path).is_some(), kind, "{path}");
        }
    }

    #[test]
    fn the_policy_shuts_everything_but_its_own_folder() {
        let csp = frame_csp("http://platypus-plugin.localhost", "demo");
        assert!(csp.starts_with("sandbox allow-scripts;"), "{csp}");
        assert!(!csp.contains("allow-same-origin"), "{csp}");
        assert!(csp.contains("default-src 'none'"), "{csp}");
        assert!(csp.contains("connect-src 'none'"), "{csp}");
        assert!(csp.contains("frame-src 'none'"), "{csp}");
        assert!(csp.contains("form-action 'none'"), "{csp}");
        assert!(csp.contains("base-uri 'none'"), "{csp}");
        assert!(!csp.contains("unsafe-eval"), "{csp}");
        assert!(
            csp.contains("script-src 'unsafe-inline' http://platypus-plugin.localhost/demo/;"),
            "{csp}"
        );
    }

    #[test]
    fn the_path_of_a_request_names_a_plugin_and_a_file() {
        assert_eq!(frame_target("/demo/ui/a.html"), Some(("demo", "ui/a.html")));
        assert_eq!(frame_target("/demo"), None);
        assert_eq!(frame_target("demo/a"), None);
    }

    #[test]
    fn the_bootstrap_comes_first_and_keeps_the_doctype() {
        let script = format!("<script>{BOOTSTRAP}</script>");
        let shown = |html: &str| String::from_utf8(with_bootstrap(html.as_bytes())).unwrap();
        assert_eq!(
            shown("<!doctype html><p>x"),
            format!("<!doctype html>{script}<p>x")
        );
        assert_eq!(
            shown("\n <!DOCTYPE html>\n<p>x"),
            format!("\n <!DOCTYPE html>{script}\n<p>x")
        );
        assert_eq!(shown("<p>x"), format!("{script}<p>x"));
        assert_eq!(shown("\u{feff}<p>x"), format!("{script}<p>x"));
        // A page that never says where its doctype ends is still given the script first.
        assert_eq!(shown("<!doctype html"), format!("{script}<!doctype html"));
    }

    #[test]
    fn a_request_is_one_the_app_knows() {
        let ok = |v: Value| FrameRequest::parse(v).unwrap();
        assert_eq!(
            ok(json!({"name": "notify", "params": {"text": "hi"}})),
            FrameRequest::Notify {
                text: "hi".to_owned()
            }
        );
        assert_eq!(
            ok(json!({"name": "state_get", "params": {}})),
            FrameRequest::StateGet {}
        );
        for bad in [
            json!({"name": "run_sql", "params": {"sql": "DROP TABLE t"}}),
            json!({"name": "connections", "params": {}}),
            json!({"name": "notify", "params": {"text": "x", "html": "<b>x</b>"}}),
            json!({"name": "notify"}),
            json!("notify"),
            json!(null),
        ] {
            let e = FrameRequest::parse(bad.clone()).unwrap_err();
            assert!(matches!(e, PluginError::Denied(_)), "{bad}: {e}");
        }
        let big = json!({"name": "notify", "params": {"text": "x".repeat(MAX_REQUEST)}});
        let e = FrameRequest::parse(big).unwrap_err();
        assert!(e.to_string().contains("larger than"), "{e}");
    }

    #[test]
    fn the_rate_refuses_a_flood_and_recovers() {
        let start = Instant::now();
        let mut rate = Rate::full(start);
        let taken = (0..1000).filter(|_| rate.take(start)).count();
        assert_eq!(taken, 100);
        assert!(!rate.take(start));
        assert!(rate.take(start + Duration::from_secs(1)));
    }

    #[test]
    fn a_notice_is_one_short_line() {
        assert_eq!(clean_notice(" a\nb\u{0}c "), "a b c");
        assert_eq!(clean_notice(&"x".repeat(9999)).chars().count(), MAX_NOTICE);
    }
}
