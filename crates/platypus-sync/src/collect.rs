//! Between this machine's data and sync items: what is collected (settings that are not
//! machine specific, connections without credentials, projects, scripts) and how a merged
//! result is applied back.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use platypus_store::{
    ConnectionStore, LINKS_FILE, NetworkProfile, Project, ProjectFolder, SyncBatch, list_scripts,
    read_script_links, resolve_script,
};
use serde_json::Value;
use uuid::Uuid;

use crate::merge::Change;
use crate::model::{
    BaseEntry, CONNECTION, Item, PROFILE, PROJECT, RAIL_FOLDER, SCRIPT, SETTING, fingerprint,
};
use crate::portable::{ShareConnection, ShareProfile};
use crate::{Result, SyncError};

/// Setting sections that describe this machine, not the user: the window, the drivers
/// installed here, interface layout, and the sync settings themselves.
const MACHINE_SECTIONS: [&str; 6] = ["window", "drivers", "ui", "paths", "sync", "shared"];

/// Single settings that depend on the operating system or the screen.
const MACHINE_SETTINGS: [&str; 3] = ["appearance.zoom", "appearance.title_bar", "appearance.tray"];

/// Whether a setting stays on this machine: it is in a machine section, is listed in
/// `MACHINE_SETTINGS`, or names a file or folder (`…_path`, `…_dir`, `…_file`, `…_folder`).
#[must_use]
pub fn is_machine_setting(key: &str) -> bool {
    let section = key.split('.').next().unwrap_or(key);
    let last = key.rsplit('.').next().unwrap_or(key);
    MACHINE_SECTIONS.contains(&section)
        || MACHINE_SETTINGS.contains(&key)
        || ["path", "dir", "file", "folder", "directory"]
            .iter()
            .any(|w| last == *w || last.ends_with(&format!("_{w}")))
}

/// Settings as `setting/<dotted key>` values, without the machine-specific ones.
#[must_use]
pub fn setting_values(table: &toml::Table) -> BTreeMap<String, Value> {
    fn walk(table: &toml::Table, prefix: &str, out: &mut BTreeMap<String, Value>) {
        for (name, value) in table {
            let key = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}.{name}")
            };
            match value {
                toml::Value::Table(t) => walk(t, &key, out),
                other => {
                    if !is_machine_setting(&key)
                        && let Ok(json) = serde_json::to_value(other)
                    {
                        out.insert(format!("{SETTING}{key}"), json);
                    }
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(table, "", &mut out);
    out
}

/// Connections, network profiles, projects and rail folders as items' values. Shared
/// projects are not here: they travel through their own folder.
#[must_use]
pub fn workspace_values(store: &ConnectionStore) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let json = |v: &dyn erased::Json| v.to_json();
    for c in store.list() {
        out.insert(
            format!("{CONNECTION}{}", c.id),
            json(&ShareConnection::from_saved(c, true)),
        );
    }
    for p in store.network_profiles() {
        out.insert(
            format!("{PROFILE}{}", p.id),
            json(&ShareProfile::from_saved(p, true)),
        );
    }
    let shared: HashSet<Uuid> = store
        .projects()
        .iter()
        .filter(|p| p.shared.is_some())
        .map(|p| p.id)
        .collect();
    for p in store.projects().iter().filter(|p| p.shared.is_none()) {
        out.insert(format!("{PROJECT}{}", p.id), json(p));
    }
    for f in store.project_folders() {
        let mut f = f.clone();
        f.projects.retain(|id| !shared.contains(id));
        if !f.projects.is_empty() {
            out.insert(format!("{RAIL_FOLDER}{}", f.id), json(&f));
        }
    }
    out
}

/// Serialising without naming each type twice.
mod erased {
    use serde::Serialize;
    use serde_json::Value;

    pub trait Json {
        fn to_json(&self) -> Value;
    }

    impl<T: Serialize> Json for T {
        fn to_json(&self) -> Value {
            serde_json::to_value(self).unwrap_or(Value::Null)
        }
    }
}

/// The largest script that is synchronised.
pub const MAX_SCRIPT_BYTES: u64 = 4 * 1024 * 1024;

/// The scripts under `base/<scope>` for each scope (`all`, or a project's id), as
/// `script/<scope>/<path>` values `{ "text", "connection" }`, with their files' modification
/// times (milliseconds) as the hint for when they changed.
#[must_use]
pub fn script_values(
    base: &Path,
    scopes: &[String],
) -> (BTreeMap<String, Value>, BTreeMap<String, i64>) {
    let mut values = BTreeMap::new();
    let mut times = BTreeMap::new();
    for scope in scopes {
        let root = base.join(scope);
        for entry in list_scripts(&root).into_iter().filter(|e| !e.folder) {
            let Some(path) = resolve_script(&root, &entry.path) else {
                continue;
            };
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if meta.len() > MAX_SCRIPT_BYTES {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let key = format!("{SCRIPT}{scope}/{}", entry.path);
            if let Some(ms) = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|d| i64::try_from(d.as_millis()).ok())
            {
                times.insert(key.clone(), ms);
            }
            values.insert(
                key,
                serde_json::json!({ "text": text, "connection": entry.connection }),
            );
        }
    }
    (values, times)
}

/// This machine's items: each value stamped with the time it last changed. A value as it was
/// at the last sync keeps its stamp; one that differs gets its `hint` (a file's time) or
/// `now`. What was synchronised and is gone becomes a tombstone.
#[must_use]
pub fn local_items(
    current: &BTreeMap<String, Value>,
    hints: &BTreeMap<String, i64>,
    base: &BTreeMap<String, BaseEntry>,
    machine: &str,
    now: i64,
) -> BTreeMap<String, Item> {
    let mut out = BTreeMap::new();
    for (key, value) in current {
        let unchanged = base
            .get(key)
            .filter(|b| !b.deleted && b.fingerprint == fingerprint(false, value));
        let item = match unchanged {
            Some(b) => Item::live(value.clone(), b.modified, &b.by),
            None => Item::live(
                value.clone(),
                hints.get(key).copied().unwrap_or(now),
                machine,
            ),
        };
        out.insert(key.clone(), item);
    }
    for (key, b) in base {
        if out.contains_key(key) {
            continue;
        }
        let tombstone = if b.deleted {
            Item::tombstone(b.modified, &b.by)
        } else {
            Item::tombstone(now, machine)
        };
        out.insert(key.clone(), tombstone);
    }
    out
}

/// A script to write or remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptOp {
    /// `all` or a project's id.
    pub scope: String,
    /// Relative to the scope's folder, with `/`.
    pub path: String,
    /// `None` deletes the script.
    pub text: Option<String>,
    pub connection: Option<Uuid>,
}

/// What a merge asks of this machine, in terms of its stores.
#[derive(Debug, Default)]
pub struct Plan {
    pub batch: SyncBatch,
    /// Settings to set (`Some`) or remove (`None`), by dotted key.
    pub settings: Vec<(String, Option<toml::Value>)>,
    pub scripts: Vec<ScriptOp>,
    /// Keys whose value could not be read (a damaged item): they are left as they are.
    pub skipped: Vec<String>,
}

fn parse<T: serde::de::DeserializeOwned>(item: &Item, key: &str, plan: &mut Plan) -> Option<T> {
    let parsed = serde_json::from_value(item.value.clone()).ok();
    if parsed.is_none() {
        plan.skipped.push(key.to_owned());
    }
    parsed
}

/// Turns the changes of a merge into store operations. The connections' private fields are
/// taken from `store`, so a change never overwrites a path or a command that stays here.
#[must_use]
pub fn plan(changes: &[Change], store: &ConnectionStore) -> Plan {
    let mut plan = Plan::default();
    for change in changes {
        let key = change.key();
        match change {
            Change::Put { item, .. } => put(&mut plan, key, item, store),
            Change::Remove { .. } => remove(&mut plan, key),
        }
    }
    plan
}

fn put(plan: &mut Plan, key: &str, item: &Item, store: &ConnectionStore) {
    if let Some(name) = key.strip_prefix(SETTING) {
        match toml::Value::try_from(item.value.clone()) {
            Ok(v) if !is_machine_setting(name) => plan.settings.push((name.to_owned(), Some(v))),
            _ => plan.skipped.push(key.to_owned()),
        }
    } else if let Some(id) = key
        .strip_prefix(CONNECTION)
        .and_then(|i| Uuid::parse_str(i).ok())
    {
        if let Some(shared) = parse::<ShareConnection>(item, key, plan) {
            plan.batch
                .connections
                .push(shared.into_saved(store.get(id)));
        }
    } else if let Some(id) = key
        .strip_prefix(PROFILE)
        .and_then(|i| Uuid::parse_str(i).ok())
    {
        if let Some(shared) = parse::<ShareProfile>(item, key, plan) {
            let existing: Option<&NetworkProfile> =
                store.network_profiles().iter().find(|p| p.id == id);
            plan.batch
                .network_profiles
                .push(shared.into_saved(existing));
        }
    } else if key.starts_with(PROJECT) {
        if let Some(mut project) = parse::<Project>(item, key, plan) {
            let existing = store.projects().iter().find(|p| p.id == project.id);
            // A project linked to a folder here stays as it is: it syncs through its folder.
            if existing.is_some_and(|p| p.shared.is_some()) {
                return;
            }
            project.shared = None;
            plan.batch.projects.push(project);
        }
    } else if key.starts_with(RAIL_FOLDER) {
        if let Some(folder) = parse::<ProjectFolder>(item, key, plan) {
            plan.batch.project_folders.push(folder);
        }
    } else if let Some(rest) = key.strip_prefix(SCRIPT) {
        script_put(plan, key, rest, item);
    }
}

fn script_put(plan: &mut Plan, key: &str, rest: &str, item: &Item) {
    let Some((scope, path)) = rest.split_once('/') else {
        plan.skipped.push(key.to_owned());
        return;
    };
    let text = item.value.get("text").and_then(Value::as_str);
    let Some(text) = text else {
        plan.skipped.push(key.to_owned());
        return;
    };
    let connection = item
        .value
        .get("connection")
        .and_then(Value::as_str)
        .and_then(|c| Uuid::parse_str(c).ok());
    plan.scripts.push(ScriptOp {
        scope: scope.to_owned(),
        path: path.to_owned(),
        text: Some(text.to_owned()),
        connection,
    });
}

fn remove(plan: &mut Plan, key: &str) {
    let id = |prefix: &str| {
        key.strip_prefix(prefix)
            .and_then(|i| Uuid::parse_str(i).ok())
    };
    if let Some(name) = key.strip_prefix(SETTING) {
        if !is_machine_setting(name) {
            plan.settings.push((name.to_owned(), None));
        }
    } else if let Some(id) = id(CONNECTION) {
        plan.batch.remove_connections.push(id);
    } else if let Some(id) = id(PROFILE) {
        plan.batch.remove_network_profiles.push(id);
    } else if let Some(id) = id(PROJECT) {
        plan.batch.remove_projects.push(id);
    } else if let Some(id) = id(RAIL_FOLDER) {
        plan.batch.remove_project_folders.push(id);
    } else if let Some((scope, path)) = key.strip_prefix(SCRIPT).and_then(|r| r.split_once('/')) {
        plan.scripts.push(ScriptOp {
            scope: scope.to_owned(),
            path: path.to_owned(),
            text: None,
            connection: None,
        });
    }
}

fn io(path: &Path, e: &std::io::Error) -> SyncError {
    SyncError::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    }
}

/// Writes and removes the scripts under `base`. Only plain relative paths are accepted, so a
/// crafted item cannot reach outside the scripts folder. Returns how many were done.
pub fn apply_scripts(base: &Path, ops: &[ScriptOp]) -> Result<usize> {
    let mut done = 0;
    let mut by_scope: BTreeMap<&str, Vec<&ScriptOp>> = BTreeMap::new();
    for op in ops {
        by_scope.entry(op.scope.as_str()).or_default().push(op);
    }
    for (scope, ops) in by_scope {
        let plain_scope = scope == "all" || Uuid::parse_str(scope).is_ok();
        if !plain_scope {
            continue;
        }
        let root = base.join(scope);
        let mut links: BTreeMap<String, Uuid> = read_script_links(&root);
        let mut links_changed = false;
        for op in ops {
            let Some(path) = resolve_script(&root, &op.path).filter(|_| {
                Path::new(&op.path)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("sql"))
            }) else {
                continue;
            };
            if let Some(text) = &op.text {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| io(dir, &e))?;
                }
                std::fs::write(&path, text).map_err(|e| io(&path, &e))?;
                links_changed |= match op.connection {
                    Some(c) => links.insert(op.path.clone(), c) != Some(c),
                    None => links.remove(&op.path).is_some(),
                };
            } else {
                if let Err(e) = std::fs::remove_file(&path)
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    return Err(io(&path, &e));
                }
                links_changed |= links.remove(&op.path).is_some();
            }
            done += 1;
        }
        if links_changed {
            let file: PathBuf = root.join(LINKS_FILE);
            let text = serde_json::to_string_pretty(&links)
                .map_err(|e| SyncError::Invalid(e.to_string()))?;
            std::fs::write(&file, text).map_err(|e| io(&file, &e))?;
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn machine_settings_stay_home() {
        for key in [
            "window.width",
            "drivers.auto_update",
            "ui.settings_section",
            "appearance.zoom",
            "appearance.tray",
            "editor.export_dir",
            "results.backup_path",
            "sync.folder",
            "something.file",
        ] {
            assert!(is_machine_setting(key), "{key}");
        }
        for key in [
            "appearance.theme",
            "editor.vim",
            "safety.auto_limit",
            "shortcuts.run",
            "editor.profile_pic",
        ] {
            assert!(!is_machine_setting(key), "{key}");
        }
    }

    #[test]
    fn settings_flatten_without_the_machine_ones() {
        let table: toml::Table = toml::from_str(
            "[appearance]\ntheme = \"dark\"\nzoom = 1.2\n[drivers]\nauto_update = true\n[editor]\nvim = true\nexport_dir = \"C:/x\"\n",
        )
        .unwrap();
        let values = setting_values(&table);
        assert_eq!(
            values.keys().map(String::as_str).collect::<Vec<_>>(),
            ["setting/appearance.theme", "setting/editor.vim"]
        );
        assert_eq!(values["setting/editor.vim"], json!(true));
    }

    #[test]
    fn stamps_follow_changes_and_deletions_become_tombstones() {
        let a = json!("a");
        let b = json!("b");
        let base = BTreeMap::from([
            (
                "setting/same".to_owned(),
                BaseEntry::of(&Item::live(a.clone(), 10, "OLD")),
            ),
            (
                "setting/edited".to_owned(),
                BaseEntry::of(&Item::live(a.clone(), 10, "OLD")),
            ),
            (
                "setting/gone".to_owned(),
                BaseEntry::of(&Item::live(a.clone(), 10, "OLD")),
            ),
            (
                "setting/long_gone".to_owned(),
                BaseEntry::of(&Item::tombstone(5, "OLD")),
            ),
        ]);
        let current = BTreeMap::from([
            ("setting/same".to_owned(), a.clone()),
            ("setting/edited".to_owned(), b.clone()),
            ("setting/new".to_owned(), b.clone()),
        ]);
        let hints = BTreeMap::from([("setting/new".to_owned(), 77)]);
        let items = local_items(&current, &hints, &base, "ME", 1000);
        assert_eq!(items["setting/same"], Item::live(a, 10, "OLD"));
        assert_eq!(items["setting/edited"], Item::live(b.clone(), 1000, "ME"));
        assert_eq!(items["setting/new"], Item::live(b, 77, "ME"));
        assert_eq!(items["setting/gone"], Item::tombstone(1000, "ME"));
        assert_eq!(items["setting/long_gone"], Item::tombstone(5, "OLD"));
    }

    #[test]
    fn scripts_round_trip_through_items() {
        let dir = tempfile::tempdir().unwrap();
        let project = Uuid::new_v4();
        let c = Uuid::new_v4();
        let ops = [
            ScriptOp {
                scope: "all".into(),
                path: "reports/m.sql".into(),
                text: Some("select 1".into()),
                connection: Some(c),
            },
            ScriptOp {
                scope: project.to_string(),
                path: "x.sql".into(),
                text: Some("select 2".into()),
                connection: None,
            },
        ];
        assert_eq!(apply_scripts(dir.path(), &ops).unwrap(), 2);
        let scopes = ["all".to_owned(), project.to_string()];
        let (values, times) = script_values(dir.path(), &scopes);
        assert_eq!(
            values["script/all/reports/m.sql"],
            json!({"text": "select 1", "connection": c})
        );
        assert_eq!(
            values[&format!("script/{project}/x.sql")]["text"],
            "select 2"
        );
        assert_eq!(times.len(), 2);
        // A deletion removes the file and its link.
        let delete = ScriptOp {
            scope: "all".into(),
            path: "reports/m.sql".into(),
            text: None,
            connection: None,
        };
        apply_scripts(dir.path(), &[delete]).unwrap();
        assert!(!dir.path().join("all/reports/m.sql").exists());
        assert!(read_script_links(&dir.path().join("all")).is_empty());
    }

    #[test]
    fn crafted_script_paths_stay_inside() {
        let dir = tempfile::tempdir().unwrap();
        let bad = |scope: &str, path: &str| ScriptOp {
            scope: scope.into(),
            path: path.into(),
            text: Some("x".into()),
            connection: None,
        };
        let ops = [
            bad("all", "../evil.sql"),
            bad("all", "/abs.sql"),
            bad("..", "x.sql"),
            bad("all", ".links.json"),
            bad("all", "notsql.txt"),
        ];
        assert_eq!(apply_scripts(dir.path(), &ops).unwrap(), 0);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
