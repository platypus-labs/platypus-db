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

const THEME: &str = "id = \"sea\"\nname = \"Sea\"\nversion = \"1.0.0\"\napi = 2\nkinds = [\"theme\"]\n[theme]\nname = \"Sea\"\ndark = { bg = \"#0b1d26\" }\n";

#[tokio::test(flavor = "multi_thread")]
async fn a_theme_installs_without_a_module() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let root = tempfile::tempdir().unwrap();

    // A folder without a module.
    let dir = root.path().join("sea");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), THEME).unwrap();
    let inspected = plugins.inspect(&dir).await.unwrap();
    assert_eq!(inspected.description, Description::default());
    assert!(inspected.manifest.theme.is_some());
    let installed = plugins.install(&dir, &inspected.hash).await.unwrap();
    assert!(installed.enabled);
    let home = data.path().join("plugins/sea/1.0.0");
    assert!(home.join(MANIFEST_FILE).exists());
    assert!(!home.join(MODULE_FILE).exists());
    assert_eq!(plugins.list().unwrap().len(), 1);
    // Nothing of it runs.
    let e = plugins.run_action("sea", "x", json!({})).await.unwrap_err();
    assert!(e.to_string().contains("no module"), "{e}");

    // A module beside the manifest of a theme is not copied.
    std::fs::write(dir.join(MODULE_FILE), b"anything").unwrap();
    let inspected = plugins.inspect(&dir).await.unwrap();
    plugins.install(&dir, &inspected.hash).await.unwrap();
    assert!(!home.join(MODULE_FILE).exists());

    // In a .zip too.
    let zip_path = root.path().join("sea.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
    zip.start_file(MANIFEST_FILE, zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(THEME.as_bytes()).unwrap();
    zip.finish().unwrap();
    let inspected = plugins.inspect(&zip_path).await.unwrap();
    plugins.install(&zip_path, &inspected.hash).await.unwrap();

    // Everything else still needs its module.
    let zip_path = root.path().join("actions.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
    zip.start_file(MANIFEST_FILE, zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(manifest("act", "1", "[\"actions\"]", "").as_bytes())
        .unwrap();
    zip.finish().unwrap();
    let e = plugins.inspect(&zip_path).await.unwrap_err().to_string();
    assert!(e.contains(MODULE_FILE), "{e}");
}

fn themed(fonts: &str) -> String {
    format!(
        "id = \"sea\"\nname = \"Sea\"\nversion = \"1.0.0\"\napi = 2\nkinds = [\"theme\"]\n[theme]\nname = \"Sea\"\ndark = {{ bg = \"#0b1d26\" }}\n{fonts}"
    )
}

const FONT: &[u8] = b"wOF2 a font for the tests";

fn zipped(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_theme_brings_its_fonts_from_the_package() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let root = tempfile::tempdir().unwrap();
    let manifest = themed("fonts = { mono = \"fonts/Sea.woff2\" }\n");
    let zip_path = root.path().join("sea.zip");
    std::fs::write(
        &zip_path,
        zipped(&[
            ("sea/plugin.toml", manifest.as_bytes()),
            ("sea/fonts/Sea.woff2", FONT),
        ]),
    )
    .unwrap();
    let inspected = plugins.inspect(&zip_path).await.unwrap();

    // Another font is another package: the review does not hold.
    let other = root.path().join("other.zip");
    std::fs::write(
        &other,
        zipped(&[
            ("sea/plugin.toml", manifest.as_bytes()),
            ("sea/fonts/Sea.woff2", b"wOF2 another font"),
        ]),
    )
    .unwrap();
    assert_ne!(plugins.inspect(&other).await.unwrap().hash, inspected.hash);

    plugins.install(&zip_path, &inspected.hash).await.unwrap();
    assert!(
        data.path()
            .join("plugins/sea/1.0.0/fonts/Sea.woff2")
            .exists()
    );
    assert_eq!(plugins.theme_font("sea", "mono").unwrap(), FONT);
    let e = plugins.theme_font("sea", "ui").unwrap_err().to_string();
    assert!(e.contains("no font"), "{e}");

    // Off, nothing reads it.
    plugins.set_enabled("sea", false).unwrap();
    assert!(matches!(
        plugins.theme_font("sea", "mono"),
        Err(PluginError::Disabled(_))
    ));

    // A folder too.
    let dir = root.path().join("folder");
    std::fs::create_dir_all(dir.join("fonts")).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), &manifest).unwrap();
    std::fs::write(dir.join("fonts/Sea.woff2"), FONT).unwrap();
    let inspected = plugins.inspect(&dir).await.unwrap();
    plugins.install(&dir, &inspected.hash).await.unwrap();
    plugins.set_enabled("sea", true).unwrap();
    assert_eq!(plugins.theme_font("sea", "mono").unwrap(), FONT);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_theme_is_refused_with_what_it_cannot_hold() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let root = tempfile::tempdir().unwrap();
    let inspect = async |name: &str, manifest: &str, files: &[(&str, &[u8])]| {
        let path = root.path().join(name);
        let mut all = vec![("plugin.toml", manifest.as_bytes())];
        all.extend_from_slice(files);
        std::fs::write(&path, zipped(&all)).unwrap();
        plugins.inspect(&path).await
    };
    let with_font = themed("fonts = { mono = \"a.woff2\" }\n");
    let big = vec![b'w'; usize::try_from(theme::MAX_FONT).unwrap() + 1];
    let mut oversize = b"wOF2".to_vec();
    oversize.extend_from_slice(&big);
    for (name, manifest, files, why) in [
        (
            "escape.zip",
            themed("").replace("#0b1d26", "#fff; background: url(x)"),
            vec![],
            "not a colour",
        ),
        (
            "url.zip",
            themed("").replace("#0b1d26", "url(https://example.com/x)"),
            vec![],
            "not a colour",
        ),
        (
            "token.zip",
            themed("").replace("bg =", "glow ="),
            vec![],
            "unknown colour dark.glow",
        ),
        (
            "family.zip",
            themed("fonts = { mono = \"Fira Code\" }\n"),
            vec![],
            "not a",
        ),
        ("missing.zip", with_font.clone(), vec![], "no a.woff2"),
        (
            "html.zip",
            with_font.clone(),
            vec![("a.woff2", b"<html>".as_slice())],
            "not a font",
        ),
        (
            "large.zip",
            with_font.clone(),
            vec![("a.woff2", oversize.as_slice())],
            "larger than",
        ),
    ] {
        let e = inspect(name, &manifest, &files).await.unwrap_err();
        assert!(e.to_string().contains(why), "{name}: {e}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_manual_install_refuses_paths_that_climb_out_too() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("up.zip");
    std::fs::write(
        &path,
        zipped(&[
            ("plugin.toml", themed("").as_bytes()),
            ("../../evil.txt", b"x"),
        ]),
    )
    .unwrap();
    let e = plugins.inspect(&path).await.unwrap_err().to_string();
    assert!(e.contains("climbs out"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_leaves_no_files_and_keeps_settings_only_when_asked() {
    let data = tempfile::tempdir().unwrap();
    let secrets = Arc::new(platypus_store::MemorySecrets::default());
    let plugins = Plugins::new(data.path()).with_secrets(secrets);
    let root = tempfile::tempdir().unwrap();
    let source = package(
        root.path(),
        "cfg",
        &abi2_actions("1", URL_AND_TOKEN),
        DESCRIBE_ACTIONS,
        COPY,
    );
    let set_up = async || {
        install(&plugins, &source).await.unwrap();
        plugins
            .set_setting("cfg", "url", &json!("https://x"))
            .unwrap();
        plugins
            .set_setting("cfg", "token", &json!("s3cret"))
            .unwrap();
    };
    let home = data.path().join("plugins/cfg");
    let file = data.path().join("plugins/settings.toml");

    // Kept when chosen: the values, for a later install; never the secret.
    set_up().await;
    plugins.remove_keeping_settings("cfg").unwrap();
    assert!(!home.exists());
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("https://x")
    );
    install(&plugins, &source).await.unwrap();
    let states = plugins.settings_of("cfg").unwrap();
    assert_eq!(states[0].value, Some(json!("https://x")));
    assert!(!states[1].set, "the secret left the keychain");

    // Otherwise everything goes, down to the file with the last plugin's values.
    plugins
        .set_setting("cfg", "token", &json!("s3cret"))
        .unwrap();
    plugins.remove("cfg").unwrap();
    assert!(!home.exists());
    assert!(!file.exists());
    install(&plugins, &source).await.unwrap();
    assert!(plugins.settings_of("cfg").unwrap().iter().all(|s| !s.set));

    // The values of another plugin stay.
    plugins
        .set_setting("cfg", "url", &json!("https://x"))
        .unwrap();
    std::fs::write(
        &file,
        "version = 1\n[other]\nkey = 1\n[cfg]\nurl = \"https://x\"\n",
    )
    .unwrap();
    plugins.remove("cfg").unwrap();
    let left = std::fs::read_to_string(&file).unwrap();
    assert!(left.contains("other") && !left.contains("cfg"), "{left}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_update_says_what_it_asks_anew() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let root = tempfile::tempdir().unwrap();
    let first = actions_package(root.path(), "1.0.0", "");
    // A first install asks for all it declares.
    let inspected = plugins.inspect(&first).await.unwrap();
    assert!(inspected.added.is_empty());
    plugins.install(&first, &inspected.hash).await.unwrap();

    let second = actions_package(
        root.path(),
        "2.0.0",
        "[permissions]\nclipboard = true\nhttp = [\"api.example.com\"]\n",
    );
    let inspected = plugins.inspect(&second).await.unwrap();
    assert_eq!(inspected.replaces.as_deref(), Some("1.0.0"));
    assert!(inspected.added.clipboard);
    assert_eq!(inspected.added.http, ["api.example.com"]);
    // What is not installed again stays as it was approved.
    assert!(!plugins.list().unwrap()[0].approved.clipboard);
    let installed = plugins.install(&second, &inspected.hash).await.unwrap();
    assert!(installed.approved.clipboard);

    // The same permissions again ask nothing new.
    let inspected = plugins.inspect(&second).await.unwrap();
    assert!(inspected.added.is_empty());
}

fn abi2_actions(version: &str, settings: &str) -> String {
    format!(
        "id = \"cfg\"\nname = \"Cfg\"\nversion = \"{version}\"\napi = 2\nkinds = [\"actions\"]\n{settings}"
    )
}

const URL_AND_TOKEN: &str = "[[settings]]\nkey = \"url\"\nlabel = \"URL\"\ntype = \"text\"\ndefault = \"https://default\"\n[[settings]]\nkey = \"token\"\nlabel = \"Token\"\ntype = \"secret\"\n";

fn memory_plugins(data: &Path) -> Plugins {
    Plugins::new(data).with_secrets(Arc::new(platypus_store::MemorySecrets::default()))
}

#[tokio::test(flavor = "multi_thread")]
async fn saves_and_forgets_settings_with_the_plugin() {
    let data = tempfile::tempdir().unwrap();
    let plugins = memory_plugins(data.path());
    let root = tempfile::tempdir().unwrap();
    let source = package(
        root.path(),
        "cfg",
        &abi2_actions("1", URL_AND_TOKEN),
        DESCRIBE_ACTIONS,
        COPY,
    );
    install(&plugins, &source).await.unwrap();

    let states = plugins.settings_of("cfg").unwrap();
    assert!(states.iter().all(|s| !s.set && s.value.is_none()));
    plugins
        .set_setting("cfg", "url", &json!("https://x"))
        .unwrap();
    plugins
        .set_setting("cfg", "token", &json!("s3cret"))
        .unwrap();
    let states = plugins.settings_of("cfg").unwrap();
    assert_eq!(states[0].value, Some(json!("https://x")));
    assert!(states[1].set);
    assert_eq!(states[1].value, None);
    let file = std::fs::read_to_string(data.path().join("plugins/settings.toml")).unwrap();
    assert!(
        file.contains("https://x") && !file.contains("s3cret"),
        "{file}"
    );

    // Refused: a key it does not declare, a value that does not fit, another plugin's.
    let e = plugins
        .set_setting("cfg", "other", &json!("x"))
        .unwrap_err();
    assert!(e.to_string().contains("no setting"), "{e}");
    let e = plugins.set_setting("cfg", "url", &json!(3)).unwrap_err();
    assert!(e.to_string().contains("not a valid"), "{e}");
    assert!(matches!(
        plugins.set_setting("gone", "url", &json!("x")),
        Err(PluginError::NotInstalled(_))
    ));

    // An update without a field forgets it, and what remains stays.
    let source = package(
        root.path(),
        "cfg2",
        &abi2_actions(
            "2",
            "[[settings]]\nkey = \"url\"\nlabel = \"URL\"\ntype = \"text\"\n",
        ),
        DESCRIBE_ACTIONS,
        COPY,
    );
    install(&plugins, &source).await.unwrap();
    let states = plugins.settings_of("cfg").unwrap();
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].value, Some(json!("https://x")));

    // Removing it forgets the rest; a new install starts clean.
    plugins.remove("cfg").unwrap();
    let source = package(
        root.path(),
        "cfg3",
        &abi2_actions("3", URL_AND_TOKEN),
        DESCRIBE_ACTIONS,
        COPY,
    );
    install(&plugins, &source).await.unwrap();
    let states = plugins.settings_of("cfg").unwrap();
    assert!(states.iter().all(|s| !s.set), "{states:?}");
}

/// The list of plugins as JSON, changed by `edit` and written back.
fn edit_state(data: &Path, edit: impl FnOnce(&mut Value)) {
    let path = data.join("plugins").join(STATE_FILE);
    let mut state: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    edit(&mut state);
    std::fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_list_and_the_files_must_agree() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    install(
        &plugins,
        &actions_package(data.path(), "1.0.0", "[permissions]\nclipboard = true\n"),
    )
    .await
    .unwrap();
    let file = data.path().join("plugins/rows/1.0.0").join(MANIFEST_FILE);
    let original = std::fs::read_to_string(&file).unwrap();

    // The files of another plugin, or of another version, do not get its approved permissions.
    for other in [
        original.replace("id = \"rows\"", "id = \"other\""),
        original.replace("version = \"1.0.0\"", "version = \"9\""),
    ] {
        std::fs::write(&file, other).unwrap();
        let list = plugins.list().unwrap();
        assert!(list[0].broken, "{list:?}");
        assert!(plugins.run_action("rows", "copy", json!({})).await.is_err());
        assert!(plugins.require("rows", Capability::Hooks).is_err());
    }
    std::fs::write(&file, &original).unwrap();
    assert!(!plugins.list().unwrap()[0].broken);
    plugins.run_action("rows", "copy", json!({})).await.unwrap();

    // An entry written before the names were this strict, where `rows.` is the folder of `rows` on
    // Windows: not listed as working, and removing it leaves the folder of `rows`.
    edit_state(data.path(), |state| {
        state["plugins"]["rows."] = state["plugins"]["rows"].clone();
    });
    let list = plugins.list().unwrap();
    assert!(
        list.iter().any(|p| p.manifest.id == "rows." && p.broken),
        "{list:?}"
    );
    assert!(
        plugins
            .run_action("rows.", "copy", json!({}))
            .await
            .is_err()
    );
    plugins.remove("rows.").unwrap();
    assert!(file.is_file());
    assert_eq!(plugins.list().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_package_cannot_be_named_after_what_the_folder_holds() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    install(&plugins, &actions_package(data.path(), "1.0.0", ""))
        .await
        .unwrap();
    for (name, id, version) in [
        ("a", "plugins.json", "1"),
        ("b", "settings.toml", "1"),
        ("c", "rows.", "1"),
        ("d", "nul", "1"),
        ("e", "rows", "1.0.0."),
    ] {
        let source = package(
            data.path(),
            name,
            &manifest(id, version, "[\"actions\"]", ""),
            DESCRIBE_ACTIONS,
            COPY,
        );
        assert!(plugins.inspect(&source).await.is_err(), "{id} {version}");
    }
    assert_eq!(plugins.list().unwrap().len(), 1);
    assert!(data.path().join("plugins/rows/1.0.0").is_dir());
}

/// A plugin `cfg` with a text and a secret, both set, and what its pages kept.
async fn configured(data: &Path, root: &Path) -> Plugins {
    let plugins = memory_plugins(data);
    let source = package(
        root,
        "cfg",
        &abi2_actions("1", URL_AND_TOKEN),
        DESCRIBE_ACTIONS,
        COPY,
    );
    install(&plugins, &source).await.unwrap();
    plugins
        .set_setting("cfg", "url", &json!("https://x"))
        .unwrap();
    plugins
        .set_setting("cfg", "token", &json!("s3cret"))
        .unwrap();
    let state = data.join("plugins/.frame-state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("cfg.json"), "{}").unwrap();
    plugins
}

/// Whether a new install of `cfg` finds the secret that was set.
async fn secret_survives(plugins: &Plugins, root: &Path) -> bool {
    let source = package(
        root,
        "again",
        &abi2_actions("9", URL_AND_TOKEN),
        DESCRIBE_ACTIONS,
        COPY,
    );
    install(plugins, &source).await.unwrap();
    plugins.settings_of("cfg").unwrap()[1].set
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_removal_still_forgets_everything_and_says_so() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let plugins = configured(data.path(), root.path()).await;
    // A file where its folder should be: it cannot be deleted as one.
    let home = data.path().join("plugins/cfg");
    std::fs::remove_dir_all(&home).unwrap();
    std::fs::write(&home, "in the way").unwrap();

    let e = plugins.remove("cfg").unwrap_err();
    assert!(matches!(e, PluginError::PartlyRemoved(_)), "{e}");
    assert!(e.to_string().contains("its folder"), "{e}");
    assert_eq!(plugins.list().unwrap().len(), 0);
    assert!(!data.path().join("plugins/.frame-state/cfg.json").exists());
    let file =
        std::fs::read_to_string(data.path().join("plugins/settings.toml")).unwrap_or_default();
    assert!(!file.contains("https://x"), "{file}");
    assert!(matches!(
        plugins.remove("cfg"),
        Err(PluginError::NotInstalled(_))
    ));

    std::fs::remove_file(&home).unwrap();
    assert!(!secret_survives(&plugins, root.path()).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_whose_files_are_gone_is_listed_and_removable() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let plugins = configured(data.path(), root.path()).await;
    std::fs::remove_dir_all(data.path().join("plugins/cfg")).unwrap();

    let list = plugins.list().unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].broken);
    assert_eq!(list[0].manifest.id, "cfg");
    assert_eq!(list[0].manifest.version, "1");
    assert_eq!(list[0].description, Description::default());
    assert!(plugins.run_action("cfg", "copy", json!({})).await.is_err());

    // Its manifest is gone, and its secret goes all the same.
    plugins.remove("cfg").unwrap();
    assert_eq!(plugins.list().unwrap().len(), 0);
    assert!(!secret_survives(&plugins, root.path()).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reinstall_that_fails_leaves_the_version_it_found() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let first = actions_package(data.path(), "1.0.0", "");
    install(&plugins, &first).await.unwrap();
    let kept = data.path().join("plugins/rows/1.0.0").join(MODULE_FILE);
    let before = std::fs::read(&kept).unwrap();

    let changed = package(
        data.path(),
        "changed",
        &manifest("rows", "1.0.0", "[\"actions\"]", ""),
        DESCRIBE_ACTIONS,
        r#"{"ok":{"kind":"copy","text":"other"}}"#,
    );
    // The list cannot be written: a folder is in the way of its temporary file.
    let blocker = data.path().join("plugins").join(STATE_TMP);
    std::fs::create_dir_all(&blocker).unwrap();
    assert!(install(&plugins, &changed).await.is_err());
    assert_eq!(std::fs::read(&kept).unwrap(), before);
    assert!(!plugins.list().unwrap()[0].broken);
    let left: Vec<_> = std::fs::read_dir(data.path().join("plugins/rows"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(left, ["1.0.0"], "no temporary folder stays");

    // A new version's failure leaves the old one too.
    let newer = actions_package(data.path(), "2.0.0", "");
    assert!(install(&plugins, &newer).await.is_err());
    assert!(kept.is_file());
    assert!(!data.path().join("plugins/rows/2.0.0").exists());
    assert_eq!(plugins.list().unwrap()[0].manifest.version, "1.0.0");

    std::fs::remove_dir(&blocker).unwrap();
    install(&plugins, &changed).await.unwrap();
    assert_ne!(std::fs::read(&kept).unwrap(), before);
    assert!(!plugins.list().unwrap()[0].broken);
}

#[test]
fn a_swap_that_cannot_finish_puts_the_old_folder_back() {
    let home = tempfile::tempdir().unwrap();
    let target = home.path().join("1.0.0");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("kept"), "x").unwrap();
    assert!(swap_in(&home.path().join("missing"), &target, home.path()).is_err());
    assert!(target.join("kept").is_file());

    let new = home.path().join("new");
    std::fs::create_dir(&new).unwrap();
    std::fs::write(new.join("fresh"), "y").unwrap();
    let aside = swap_in(&new, &target, home.path()).unwrap();
    assert!(target.join("fresh").is_file());
    swap_back(&target, aside.as_deref());
    assert!(target.join("kept").is_file());
    assert!(!target.join("fresh").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_update_keeps_a_plugin_turned_off() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    install(&plugins, &actions_package(data.path(), "1.0.0", ""))
        .await
        .unwrap();
    plugins.set_enabled("rows", false).unwrap();
    let installed = install(&plugins, &actions_package(data.path(), "1.1.0", ""))
        .await
        .unwrap();
    assert!(!installed.enabled);
    assert!(!plugins.list().unwrap()[0].enabled);
    plugins.set_enabled("rows", true).unwrap();
    let installed = install(&plugins, &actions_package(data.path(), "1.2.0", ""))
        .await
        .unwrap();
    assert!(installed.enabled);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_field_that_changes_kind_leaves_nothing_behind() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let plugins = memory_plugins(data.path());
    let field =
        |kind: &str| format!("[[settings]]\nkey = \"value\"\nlabel = \"V\"\ntype = \"{kind}\"\n");
    let version = async |number: &str, kind: &str| {
        let source = package(
            root.path(),
            number,
            &abi2_actions(number, &field(kind)),
            DESCRIBE_ACTIONS,
            COPY,
        );
        install(&plugins, &source).await.unwrap();
    };
    let file = data.path().join("plugins/settings.toml");

    // Text that becomes a secret: its plain text does not stay in the file.
    version("1", "text").await;
    plugins
        .set_setting("cfg", "value", &json!("plain words"))
        .unwrap();
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("plain words")
    );
    version("2", "secret").await;
    assert!(
        !std::fs::read_to_string(&file)
            .unwrap_or_default()
            .contains("plain words")
    );

    // A secret that becomes text: nothing stays in the keychain, which shows when it is one again.
    plugins
        .set_setting("cfg", "value", &json!("s3cret"))
        .unwrap();
    version("3", "text").await;
    version("4", "secret").await;
    assert!(!plugins.settings_of("cfg").unwrap()[0].set);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_manual_install_over_a_signed_one_says_so_and_clears_what_it_was_given() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let plugins = configured(data.path(), root.path()).await;
    let state = data.path().join("plugins/.frame-state/cfg.json");
    let signer = Signer {
        key: "key-a".to_owned(),
        name: "Jane".to_owned(),
    };
    let source = package(
        root.path(),
        "2",
        &abi2_actions("2", URL_AND_TOKEN),
        DESCRIBE_ACTIONS,
        COPY,
    );
    let hash = read_package(&source).unwrap().hash;

    // Nothing changes while nobody signs: both by hand.
    assert_eq!(plugins.inspect(&source).await.unwrap().signer_change, None);
    plugins.install(&source, &hash).await.unwrap();
    assert!(plugins.settings_of("cfg").unwrap()[1].set);
    assert!(state.is_file());

    // From nobody to a key: said in the review, and all of it goes.
    let inspected = plugins
        .inspect_signed(&source, Some(&signer))
        .await
        .unwrap();
    assert_eq!(
        inspected.signer_change,
        Some(SignerChange {
            from: None,
            to: Some("Jane".to_owned())
        })
    );
    plugins
        .install_signed(&source, &hash, Some(signer.clone()))
        .await
        .unwrap();
    assert!(!plugins.settings_of("cfg").unwrap()[1].set);
    assert!(!state.exists());
    // What is not a secret stays.
    assert_eq!(
        plugins.settings_of("cfg").unwrap()[0].value,
        Some(json!("https://x"))
    );

    // The same key under another name is the same signer.
    plugins
        .set_setting("cfg", "token", &json!("again"))
        .unwrap();
    let renamed = Signer {
        key: signer.key.clone(),
        name: "J.".to_owned(),
    };
    assert_eq!(
        plugins
            .inspect_signed(&source, Some(&renamed))
            .await
            .unwrap()
            .signer_change,
        None
    );
    plugins
        .install_signed(&source, &hash, Some(renamed))
        .await
        .unwrap();
    assert!(plugins.settings_of("cfg").unwrap()[1].set);

    // From a key to a manual install: said, and cleared.
    assert_eq!(
        plugins.inspect(&source).await.unwrap().signer_change,
        Some(SignerChange {
            from: Some("J.".to_owned()),
            to: None
        })
    );
    plugins.install(&source, &hash).await.unwrap();
    assert!(!plugins.settings_of("cfg").unwrap()[1].set);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_abi_1_plugin_behaves_as_before() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let root = tempfile::tempdir().unwrap();
    let source = actions_package(root.path(), "1.0.0", "[permissions]\nclipboard = true\n");
    let installed = install(&plugins, &source).await.unwrap();
    assert_eq!(installed.manifest.api, 1);
    assert!(installed.approved.clipboard);
    let outcome = plugins
        .run_action(&installed.manifest.id, "copy", json!({}))
        .await
        .unwrap();
    assert_eq!(
        outcome,
        ActionOutcome::Copy {
            text: "a|b".to_owned()
        }
    );
    // As it was written by a version that knew nothing of the new permissions.
    let state_file = data.path().join("plugins").join(STATE_FILE);
    let mut state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_file).unwrap()).unwrap();
    let approved = state["plugins"][&installed.manifest.id]["approved"]
        .as_object_mut()
        .unwrap();
    approved.remove("hooks");
    approved.remove("tabs");
    approved.remove("grid");
    std::fs::write(&state_file, state.to_string()).unwrap();
    let listed = plugins.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].approved.clipboard && !listed[0].approved.hooks);
    // Its manifest, as an install writes it, has nothing of ABI 2.
    let written = std::fs::read_to_string(
        data.path()
            .join("plugins")
            .join(&installed.manifest.id)
            .join("1.0.0")
            .join(MANIFEST_FILE),
    )
    .unwrap();
    assert!(
        !written.contains("settings") && !written.contains("[theme"),
        "{written}"
    );
    // No hook, no tabs, no grid, and no settings.
    assert!(matches!(
        plugins.require(&installed.manifest.id, Capability::Tabs),
        Err(PluginError::Denied(_))
    ));
    assert!(matches!(
        plugins.require(&installed.manifest.id, Capability::Grid),
        Err(PluginError::Denied(_))
    ));
    assert_eq!(plugins.settings_of(&installed.manifest.id).unwrap(), []);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denied_capability_is_refused_by_name() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let root = tempfile::tempdir().unwrap();
    let source = actions_package(root.path(), "1.0.0", "");
    let installed = install(&plugins, &source).await.unwrap();
    let id = &installed.manifest.id;
    for capability in [Capability::Hooks, Capability::Tabs, Capability::Grid] {
        let e = plugins.require(id, capability).unwrap_err();
        assert!(matches!(e, PluginError::Denied(_)), "{e}");
        assert!(e.to_string().contains(&format!("{capability:?}")), "{e}");
    }
    plugins.set_enabled(id, false).unwrap();
    assert!(matches!(
        plugins.require(id, Capability::Hooks),
        Err(PluginError::Disabled(_))
    ));
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
