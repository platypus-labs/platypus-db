//! The tab and panel of `examples/plugins/panel-demo`, installed as the app does: what is copied,
//! what is served, what its pages may ask, and all of that without its permission.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use platypus_plugins::{Capability, FrameRequest, PluginError, Plugins};
use platypus_store::MemorySecrets;
use serde_json::{Value, json};

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins")
        .join(name)
}

fn plugins(data: &Path) -> Plugins {
    Plugins::new(data).with_secrets(Arc::new(MemorySecrets::default()))
}

async fn install(plugins: &Plugins, source: &Path) -> platypus_plugins::Installed {
    let reviewed = plugins.inspect(source).await.unwrap();
    plugins.install(source, &reviewed.hash).await.unwrap()
}

/// Takes the approved permission `name` back, as a state written before it existed would.
fn revoke(data: &Path, plugin: &str, name: &str) {
    let file = data.join("plugins/plugins.json");
    let mut state: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    state["plugins"][plugin]["approved"][name] = json!(false);
    std::fs::write(&file, state.to_string()).unwrap();
}

fn request(value: Value) -> FrameRequest {
    FrameRequest::parse(value).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_only_the_files_of_its_pages() {
    let data = tempfile::tempdir().unwrap();
    let plugins = plugins(data.path());
    let installed = install(&plugins, &example("panel-demo")).await;
    assert!(installed.approved.allows(Capability::Tabs));

    let notes = plugins.frame_file("panel-demo", "ui/notes.html").unwrap();
    assert!(notes.mime.starts_with("text/html"));
    let text = String::from_utf8(notes.bytes).unwrap();
    assert!(text.starts_with("<!doctype html><script>"), "{text}");
    assert!(text.contains("window.platypus"));
    let script = plugins.frame_file("panel-demo", "ui/notes.js").unwrap();
    assert!(script.mime.starts_with("text/javascript"));
    assert!(
        !String::from_utf8(script.bytes)
            .unwrap()
            .contains("window.platypus = ")
    );

    for path in [
        "plugin.toml",
        "plugin.wasm",
        "src/lib.rs",
        "Cargo.toml",
        "../plugins.json",
        "ui/../plugin.toml",
        "ui/../../panel-demo/1.0.0/plugin.toml",
        "..\\plugins.json",
        "/etc/passwd",
        "C:/Windows/win.ini",
        "ui/%2e%2e/plugin.toml",
        "ui/notes.html:stream",
        "ui/missing.js",
        "ui",
        "",
    ] {
        assert!(
            matches!(
                plugins.frame_file("panel-demo", path),
                Err(PluginError::Denied(_))
            ),
            "{path}"
        );
    }
    assert!(plugins.frame_path("panel-demo", "ui/notes.html").is_ok());
    assert!(plugins.frame_path("panel-demo", "ui/notes.js").is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_the_manifest_does_not_name_is_not_served() {
    let data = tempfile::tempdir().unwrap();
    let package = tempfile::tempdir().unwrap();
    copy_example(package.path());
    std::fs::write(package.path().join("ui/hidden.html"), "<p>x").unwrap();
    let plugins = plugins(data.path());
    install(&plugins, package.path()).await;
    // Copied with the rest of `ui/`, and still not a page.
    assert!(matches!(
        plugins.frame_file("panel-demo", "ui/hidden.html"),
        Err(PluginError::Denied(_))
    ));
}

fn copy_example(to: &Path) {
    let from = example("panel-demo");
    std::fs::create_dir_all(to.join("ui")).unwrap();
    for file in ["plugin.toml", "plugin.wasm"] {
        std::fs::copy(from.join(file), to.join(file)).unwrap();
    }
    for entry in std::fs::read_dir(from.join("ui")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), to.join("ui").join(entry.file_name())).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_review_covers_the_pages_too() {
    let data = tempfile::tempdir().unwrap();
    let package = tempfile::tempdir().unwrap();
    copy_example(package.path());
    let plugins = plugins(data.path());
    let reviewed = plugins.inspect(package.path()).await.unwrap();
    std::fs::write(package.path().join("ui/notes.js"), "evil()").unwrap();
    let e = plugins
        .install(package.path(), &reviewed.hash)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("changed since"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_package_without_its_page_is_refused() {
    let data = tempfile::tempdir().unwrap();
    let package = tempfile::tempdir().unwrap();
    copy_example(package.path());
    std::fs::remove_file(package.path().join("ui/notes.html")).unwrap();
    let e = plugins(data.path())
        .inspect(package.path())
        .await
        .unwrap_err();
    assert!(e.to_string().contains("not in the package"), "{e}");
}

/// A `.zip` of the example with `extra` files in it.
fn zipped(to: &Path, extra: &[(&str, &[u8])]) -> PathBuf {
    let path = to.join("package.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    let from = example("panel-demo");
    let mut files: Vec<(String, Vec<u8>)> = ["plugin.toml", "plugin.wasm"]
        .iter()
        .map(|f| ((*f).to_owned(), std::fs::read(from.join(f)).unwrap()))
        .collect();
    for entry in std::fs::read_dir(from.join("ui")).unwrap() {
        let entry = entry.unwrap();
        let name = format!("ui/{}", entry.file_name().to_string_lossy());
        files.push((name, std::fs::read(entry.path()).unwrap()));
    }
    for (name, bytes) in extra {
        files.push(((*name).to_owned(), bytes.to_vec()));
    }
    for (name, bytes) in files {
        zip.start_file(name, options).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}

#[tokio::test(flavor = "multi_thread")]
async fn a_zip_brings_its_pages_and_may_not_climb_out() {
    let data = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let plugins = plugins(data.path());
    let package = zipped(folder.path(), &[]);
    install(&plugins, &package).await;
    assert!(plugins.frame_file("panel-demo", "ui/notes.js").is_ok());

    let bad = zipped(folder.path(), &[("ui/../../evil.js", b"x")]);
    let e = plugins.inspect(&bad).await.unwrap_err();
    assert!(e.to_string().contains("inside the package"), "{e}");
    assert!(!data.path().join("plugins/evil.js").exists());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_link_out_of_the_package_is_refused() {
    let data = tempfile::tempdir().unwrap();
    let package = tempfile::tempdir().unwrap();
    copy_example(package.path());
    std::os::unix::fs::symlink("/etc/passwd", package.path().join("ui/passwd.js")).unwrap();
    let e = plugins(data.path())
        .inspect(package.path())
        .await
        .unwrap_err();
    assert!(e.to_string().contains("a link"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_what_its_pages_ask_within_its_permissions() {
    let data = tempfile::tempdir().unwrap();
    let plugins = plugins(data.path());
    install(&plugins, &example("panel-demo")).await;
    let ask = |value| plugins.frame_request("panel-demo", request(value));

    // Its own settings, never a secret or a key it does not declare.
    assert_eq!(
        ask(json!({"name": "setting", "params": {"key": "greeting"}}))
            .await
            .unwrap(),
        json!("Hello from the settings")
    );
    plugins
        .set_setting("panel-demo", "token", &json!("hunter2"))
        .unwrap();
    for key in ["token", "other"] {
        let e = ask(json!({"name": "setting", "params": {"key": key}}))
            .await
            .unwrap_err();
        assert!(matches!(e, PluginError::Denied(_)), "{key}: {e}");
        assert!(!e.to_string().contains("hunter2"), "{e}");
    }

    // A message is one line of plain text, from the app's side.
    assert_eq!(
        ask(json!({"name": "notify", "params": {"text": " a\n<b>b</b> "}}))
            .await
            .unwrap(),
        json!({"text": "a <b>b</b>"})
    );
    // A query is only given back to open: there is nothing here that runs one.
    assert_eq!(
        ask(json!({"name": "open_query", "params": {"text": "DROP TABLE t"}}))
            .await
            .unwrap(),
        json!({"text": "DROP TABLE t"})
    );

    // Not given these.
    for denied in [
        json!({"name": "clipboard", "params": {"text": "x"}}),
        json!({"name": "http", "params": {"url": "https://example.com"}}),
    ] {
        let e = ask(denied.clone()).await.unwrap_err();
        assert!(matches!(e, PluginError::Denied(_)), "{denied}: {e}");
    }

    // Its own module, on what the page gave it.
    assert_eq!(
        ask(json!({"name": "run_action", "params": {"action": "count-words", "context": {"text": "a b c"}}}))
            .await
            .unwrap(),
        json!({"kind": "message", "text": "3 words"})
    );
    let e = ask(json!({"name": "run_action", "params": {"action": "nope", "context": {}}}))
        .await
        .unwrap_err();
    assert!(e.to_string().contains("no action"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_a_bounded_state_that_goes_with_the_plugin() {
    let data = tempfile::tempdir().unwrap();
    let plugins = plugins(data.path());
    install(&plugins, &example("panel-demo")).await;
    let ask = |value| plugins.frame_request("panel-demo", request(value));
    let get = json!({"name": "state_get", "params": {}});
    assert_eq!(ask(get.clone()).await.unwrap(), Value::Null);
    ask(json!({"name": "state_set", "params": {"value": {"text": "kept"}}}))
        .await
        .unwrap();
    assert_eq!(ask(get.clone()).await.unwrap(), json!({"text": "kept"}));
    let e = ask(json!({"name": "state_set", "params": {"value": "x".repeat(300 << 10)}}))
        .await
        .unwrap_err();
    assert!(matches!(e, PluginError::Denied(_)), "{e}");
    assert_eq!(ask(get).await.unwrap(), json!({"text": "kept"}));

    plugins.remove("panel-demo").unwrap();
    assert!(
        !data
            .path()
            .join("plugins/.frame-state/panel-demo.json")
            .exists()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn with_http_a_page_reaches_only_its_hosts_and_with_clipboard_it_may_copy() {
    let data = tempfile::tempdir().unwrap();
    let package = tempfile::tempdir().unwrap();
    copy_example(package.path());
    let manifest = package.path().join("plugin.toml");
    let text = std::fs::read_to_string(&manifest).unwrap().replace(
        "tabs = true",
        "tabs = true\nclipboard = true\nhttp = [\"allowed.invalid\"]",
    );
    std::fs::write(&manifest, text).unwrap();
    let plugins = plugins(data.path());
    install(&plugins, package.path()).await;
    let ask = |value| plugins.frame_request("panel-demo", request(value));
    assert_eq!(
        ask(json!({"name": "clipboard", "params": {"text": "x"}}))
            .await
            .unwrap(),
        json!({"text": "x"})
    );
    let e = ask(json!({"name": "http", "params": {"url": "https://other.example.com/"}}))
        .await
        .unwrap_err();
    assert!(matches!(e, PluginError::Denied(_)), "{e}");
    let e = ask(json!({"name": "http", "params": {"url": "file:///etc/passwd"}}))
        .await
        .unwrap_err();
    assert!(e.to_string().contains("only http"), "{e}");
    // Allowed: it goes out, and this name answers nothing.
    let e = ask(json!({"name": "http", "params": {"url": "https://allowed.invalid/"}}))
        .await
        .unwrap_err();
    assert!(matches!(e, PluginError::Guest(_)), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_flood_of_requests_is_refused() {
    let data = tempfile::tempdir().unwrap();
    let plugins = plugins(data.path());
    install(&plugins, &example("panel-demo")).await;
    let mut refused = None;
    for _ in 0..1000 {
        let asked = plugins
            .frame_request("panel-demo", FrameRequest::StateGet {})
            .await;
        if let Err(e) = asked {
            refused = Some(e);
            break;
        }
    }
    let e = refused.expect("the rate stops it");
    assert!(e.to_string().contains("too many"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn without_its_permission_nothing_is_served_or_answered() {
    let data = tempfile::tempdir().unwrap();
    let plugins = plugins(data.path());
    install(&plugins, &example("panel-demo")).await;
    revoke(data.path(), "panel-demo", "tabs");

    assert!(matches!(
        plugins.require("panel-demo", Capability::Tabs),
        Err(PluginError::Denied(_))
    ));
    assert!(matches!(
        plugins.frame_file("panel-demo", "ui/notes.html"),
        Err(PluginError::Denied(_))
    ));
    assert!(matches!(
        plugins.frame_path("panel-demo", "ui/notes.html"),
        Err(PluginError::Denied(_))
    ));
    let asked = plugins
        .frame_request("panel-demo", FrameRequest::StateGet {})
        .await;
    assert!(matches!(asked, Err(PluginError::Denied(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_turned_off_serves_nothing() {
    let data = tempfile::tempdir().unwrap();
    let plugins = plugins(data.path());
    install(&plugins, &example("panel-demo")).await;
    plugins.set_enabled("panel-demo", false).unwrap();
    assert!(matches!(
        plugins.frame_file("panel-demo", "ui/notes.html"),
        Err(PluginError::Disabled(_))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_manifest_that_does_not_ask_for_tabs_is_refused() {
    let data = tempfile::tempdir().unwrap();
    let package = tempfile::tempdir().unwrap();
    copy_example(package.path());
    let manifest = package.path().join("plugin.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("tabs = true", "");
    std::fs::write(&manifest, text).unwrap();
    let e = plugins(data.path())
        .inspect(package.path())
        .await
        .unwrap_err();
    assert!(e.to_string().contains("permissions.tabs"), "{e}");
}
