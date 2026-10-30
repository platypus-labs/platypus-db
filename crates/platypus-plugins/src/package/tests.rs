use std::io::Write as _;

use serde_json::json;

use super::*;
use crate::action::ActionScope;
use crate::runtime::ACTION;
use crate::testing::{constant, guest};

const DESCRIBE_ACTIONS: &str = r#"{"ok":{"actions":[{"id":"copy","title":"Copy it","scope":"grid"},{"id":"up","title":"Up","scope":"editor","shortcut":"Mod+Alt+U"}]}}"#;
const COPY: &str = r#"{"ok":{"kind":"copy","text":"a|b"}}"#;

fn manifest(id: &str, version: &str, kinds: &str, permissions: &str) -> String {
    format!(
        "id = \"{id}\"\nname = \"Test {id}\"\nversion = \"{version}\"\napi = 1\nkinds = {kinds}\n{permissions}"
    )
}

/// A package folder in `root` with a module answering `describe` and `action`.
fn package(root: &Path, name: &str, manifest: &str, describe: &str, action: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    let body = [
        constant(DESCRIBE, 0, describe),
        constant(ACTION, 4096, action),
    ]
    .join("\n");
    std::fs::write(dir.join(MODULE_FILE), guest(&body)).unwrap();
    dir
}

/// Installs `source` as the app does once the user approved what it inspected.
async fn install(plugins: &Plugins, source: &Path) -> Result<Installed, PluginError> {
    let hash = read_package(source)?.hash;
    plugins.install(source, &hash).await
}

fn actions_package(root: &Path, version: &str, permissions: &str) -> PathBuf {
    package(
        root,
        &format!("src-{version}"),
        &manifest("rows", version, "[\"actions\"]", permissions),
        DESCRIBE_ACTIONS,
        COPY,
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn installs_lists_turns_off_and_removes() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let source = actions_package(data.path(), "1.0.0", "[permissions]\nclipboard = true\n");

    let inspected = plugins.inspect(&source).await.unwrap();
    assert_eq!(inspected.manifest.id, "rows");
    assert_eq!(inspected.description.actions.len(), 2);
    assert_eq!(inspected.description.actions[0].scope, ActionScope::Grid);
    assert_eq!(inspected.replaces, None);
    assert!(
        plugins.list().unwrap().is_empty(),
        "inspecting installs nothing"
    );

    let installed = install(&plugins, &source).await.unwrap();
    assert!(installed.enabled);
    assert!(installed.approved.clipboard);
    assert!(data.path().join("plugins/rows/1.0.0/plugin.wasm").is_file());
    assert_eq!(plugins.list().unwrap(), vec![installed]);

    let outcome = plugins
        .run_action("rows", "copy", json!({ "rows": [] }))
        .await
        .unwrap();
    assert_eq!(
        outcome,
        ActionOutcome::Copy {
            text: "a|b".to_owned()
        }
    );
    let e = plugins
        .run_action("rows", "nope", json!({}))
        .await
        .unwrap_err();
    assert!(e.to_string().contains("no action"), "{e}");

    plugins.set_enabled("rows", false).unwrap();
    assert!(!plugins.list().unwrap()[0].enabled);
    let e = plugins
        .run_action("rows", "copy", json!({}))
        .await
        .unwrap_err();
    assert!(matches!(e, PluginError::Disabled(_)), "{e}");
    plugins.set_enabled("rows", true).unwrap();

    plugins.remove("rows").unwrap();
    assert_eq!(plugins.list().unwrap().len(), 0);
    assert!(!data.path().join("plugins/rows").exists());
    let e = plugins
        .run_action("rows", "copy", json!({}))
        .await
        .unwrap_err();
    assert!(matches!(e, PluginError::NotInstalled(_)), "{e}");
    assert!(matches!(
        plugins.remove("rows"),
        Err(PluginError::NotInstalled(_))
    ));
    assert!(matches!(
        plugins.set_enabled("rows", true),
        Err(PluginError::NotInstalled(_))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn installs_only_the_package_that_was_reviewed() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let source = actions_package(data.path(), "1.0.0", "");
    let reviewed = plugins.inspect(&source).await.unwrap();
    assert_eq!(reviewed.hash.len(), 64);
    assert_eq!(
        plugins.inspect(&source).await.unwrap().hash,
        reviewed.hash,
        "the same package, the same hash"
    );
    // The package gains a permission after the review: refused, nothing installed.
    std::fs::write(
        source.join(MANIFEST_FILE),
        manifest(
            "rows",
            "1.0.0",
            "[\"actions\"]",
            "[permissions]\nclipboard = true\n",
        ),
    )
    .unwrap();
    let e = plugins.install(&source, &reviewed.hash).await.unwrap_err();
    assert!(
        e.to_string().contains("changed since it was reviewed"),
        "{e}"
    );
    assert_eq!(plugins.list().unwrap().len(), 0);
    // Another module with the same manifest is another package too.
    let other = package(
        data.path(),
        "other",
        &manifest("rows", "1.0.0", "[\"actions\"]", ""),
        DESCRIBE_ACTIONS,
        COPY,
    );
    let mut wasm = std::fs::read(other.join(MODULE_FILE)).unwrap();
    wasm.extend_from_slice(&[0, 0]);
    let first = plugins.inspect(&other).await.unwrap().hash;
    std::fs::write(other.join(MODULE_FILE), wasm).unwrap();
    assert!(plugins.install(&other, &first).await.is_err());
    // The package as it is now, reviewed again, installs.
    let again = plugins.inspect(&source).await.unwrap();
    let installed = plugins.install(&source, &again.hash).await.unwrap();
    assert!(installed.approved.clipboard);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_version_replaces_the_old_one() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    install(&plugins, &actions_package(data.path(), "1.0.0", ""))
        .await
        .unwrap();
    let newer = actions_package(data.path(), "1.1.0", "[permissions]\nclipboard = true\n");
    assert_eq!(
        plugins.inspect(&newer).await.unwrap().replaces.as_deref(),
        Some("1.0.0")
    );
    install(&plugins, &newer).await.unwrap();
    let list = plugins.list().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].manifest.version, "1.1.0");
    assert!(!data.path().join("plugins/rows/1.0.0").exists());
    assert!(data.path().join("plugins/rows/1.1.0").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn copying_needs_the_clipboard_permission() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    install(&plugins, &actions_package(data.path(), "1.0.0", ""))
        .await
        .unwrap();
    let e = plugins
        .run_action("rows", "copy", json!({}))
        .await
        .unwrap_err();
    assert!(matches!(e, PluginError::Denied(_)), "{e}");
    assert!(e.to_string().contains("clipboard"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn installs_from_a_zip() {
    let data = tempfile::tempdir().unwrap();
    let source = actions_package(data.path(), "2.0.0", "");
    let zip_path = data.path().join("rows.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
    for name in [MANIFEST_FILE, MODULE_FILE] {
        zip.start_file(
            format!("rows/{name}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&std::fs::read(source.join(name)).unwrap())
            .unwrap();
    }
    zip.finish().unwrap();
    let plugins = Plugins::new(data.path());
    let installed = install(&plugins, &zip_path).await.unwrap();
    assert_eq!(installed.manifest.version, "2.0.0");

    let not_zip = data.path().join("x.zip");
    std::fs::write(&not_zip, "nope").unwrap();
    let e = install(&plugins, &not_zip).await.unwrap_err();
    assert!(e.to_string().contains("neither a folder nor a .zip"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_packages_that_do_not_match_their_manifest() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    for (name, kinds, describe, why) in [
        ("a", "[\"driver\"]", DESCRIBE_ACTIONS, "lacks \"actions\""),
        (
            "b",
            "[\"actions\", \"driver\"]",
            DESCRIBE_ACTIONS,
            "promises a driver",
        ),
        ("c", "[\"actions\"]", r#"{"ok":{}}"#, "promises actions"),
        (
            "d",
            "[\"actions\"]",
            r#"{"ok":{"actions":[{"id":"x","title":"X","scope":"shell"}]}}"#,
            "unexpected value",
        ),
        (
            "e",
            "[\"driver\"]",
            r#"{"ok":{"driver":{"name":" "}}}"#,
            "no name",
        ),
    ] {
        let source = package(
            data.path(),
            name,
            &manifest(name, "1.0.0", kinds, ""),
            describe,
            COPY,
        );
        let e = install(&plugins, &source).await.unwrap_err();
        assert!(e.to_string().contains(why), "{name}: {e}");
    }
    let empty = data.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(install(&plugins, &empty).await.is_err());
    assert_eq!(plugins.list().unwrap().len(), 0);
    // Nothing half-written is left behind.
    assert!(!data.path().join("plugins/a").exists());
}
