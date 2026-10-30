//! End-to-end encrypted sync through a folder: settings, connections without passwords,
//! projects and scripts in one file (`platypus-sync.bin`) sealed with a sync passphrase that
//! is not the master password. The passphrase is kept in the keychain (or the vault) beside
//! the passwords; the folder, the interval and the run-at-start choice are settings under
//! `sync.` (which stay on the machine). What the last sync knew, to tell a change on both
//! sides from one on a side, is `sync-state.json` in the data folder.
//!
//! The page calls `sync_now` on demand, at start and every few minutes; the merge is in
//! `platypus-sync`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use platypus_core::Secret;
use platypus_store::{ConnectionStore, SettingsStore};
use platypus_sync::collect::{self, Plan};
use platypus_sync::merge::{Conflict, ConflictKind, Context, merge};
use platypus_sync::model::{BaseEntry, DOCUMENT_FORMAT, Document, Item, SyncState};
use platypus_sync::{SyncError, file};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use uuid::Uuid;

use super::{AppState, lock};
use crate::error::{CommandError, CommandResult};

/// Where the sync passphrase is kept, beside the passwords.
const PASSPHRASE_ID: Uuid = Uuid::from_u128(0x504c_4154_5950_5553_5359_4e43_5041_5353);
const STATE_FILE: &str = "sync-state.json";
const CONFLICTS_FILE: &str = "sync-conflicts.json";
/// How many conflicts the Sync page remembers.
const CONFLICTS_KEPT: usize = 50;
const DEFAULT_INTERVAL: u32 = 15;

/// One run at a time: the interval, the start and a click can overlap.
static RUNNING: Mutex<()> = Mutex::new(());

fn error(e: &SyncError) -> CommandError {
    let kind = match e {
        SyncError::WrongPassphrase => "sync_passphrase",
        SyncError::EmptyPassphrase => "sync_passphrase_empty",
        SyncError::Newer { .. } => "sync_newer",
        SyncError::NotSyncFile => "sync_not_file",
        SyncError::Damaged(_) => "sync_damaged",
        SyncError::Io { .. } => "sync_folder",
        SyncError::Invalid(_) => "sync",
    };
    CommandError::new(kind, e.to_string())
}

/// This computer's name, to tell where a copy of a conflicting script came from.
fn machine_name() -> String {
    ["COMPUTERNAME", "HOSTNAME", "HOST"]
        .into_iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| "this computer".to_owned())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

/// A change on both sides, as the Sync page lists it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConflictInfo {
    /// When it was found, in milliseconds since the epoch.
    pub at: i64,
    /// `script`, `connection`, `setting`, `project`…
    pub what: String,
    /// The script's path, the connection's or project's name, the setting's key.
    pub name: String,
    /// `both_edited` or `deleted_and_edited`.
    pub kind: String,
    /// The name of the copy that keeps the other version, when there is one.
    pub copy: Option<String>,
    /// The computer that made the other version.
    pub machine: String,
}

/// What the Sync page shows.
#[derive(Debug, Serialize)]
pub struct SyncStatus {
    folder: Option<String>,
    /// Whether a passphrase is saved.
    has_passphrase: bool,
    interval_minutes: u32,
    on_start: bool,
    last_sync: Option<i64>,
    machine: String,
    conflicts: Vec<ConflictInfo>,
}

/// What one run did.
#[derive(Debug, Serialize)]
pub struct SyncReport {
    /// Items taken from the file into this machine.
    applied: usize,
    /// Whether the file was written.
    written: bool,
    /// Items found damaged and left alone.
    skipped: usize,
    conflicts: Vec<ConflictInfo>,
    at: i64,
}

fn state_dir(state: &AppState) -> CommandResult<PathBuf> {
    let scripts = state
        .scripts
        .as_ref()
        .map_err(|e| CommandError::new("io", e.clone()))?;
    scripts
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| CommandError::new("io", "the data folder is not set"))
}

fn read_conflicts(dir: &Path) -> Vec<ConflictInfo> {
    std::fs::read_to_string(dir.join(CONFLICTS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_conflicts(dir: &Path, list: &[ConflictInfo]) -> CommandResult<()> {
    let text =
        serde_json::to_string_pretty(list).map_err(|e| CommandError::new("io", e.to_string()))?;
    file::write_atomic(&dir.join(CONFLICTS_FILE), text.as_bytes()).map_err(|e| error(&e))
}

fn passphrase(state: &AppState) -> CommandResult<Option<Secret>> {
    state.with_store(|s| s.secret(PASSPHRASE_ID))
}

fn folder_setting(state: &AppState) -> Option<PathBuf> {
    state
        .setting::<String>("sync.folder")
        .filter(|f| !f.trim().is_empty())
        .map(PathBuf::from)
}

fn status_of(state: &AppState) -> CommandResult<SyncStatus> {
    let dir = state_dir(state)?;
    Ok(SyncStatus {
        folder: folder_setting(state).map(|f| f.display().to_string()),
        has_passphrase: passphrase(state)?.is_some(),
        interval_minutes: state
            .setting::<u32>("sync.interval_minutes")
            .unwrap_or(DEFAULT_INTERVAL),
        on_start: state.setting::<bool>("sync.on_start").unwrap_or(true),
        last_sync: SyncState::load(&dir.join(STATE_FILE)).last_sync,
        machine: machine_name(),
        conflicts: read_conflicts(&dir),
    })
}

#[tauri::command]
pub fn sync_status(state: State<'_, AppState>) -> CommandResult<SyncStatus> {
    status_of(&state)
}

/// Chooses the folder and the passphrase of the sync, and when it runs. With a file in the
/// folder already, the passphrase must open it: a wrong one is refused and nothing changes.
/// `passphrase` is `None` to keep the saved one (when only the folder or the interval changes).
#[tauri::command]
pub fn sync_configure(
    state: State<'_, AppState>,
    folder: String,
    passphrase: Option<String>,
    interval_minutes: u32,
    on_start: bool,
) -> CommandResult<SyncStatus> {
    let folder = PathBuf::from(folder.trim());
    let saved = self::passphrase(&state)?;
    let secret = match (passphrase.filter(|p| !p.is_empty()), saved) {
        (Some(p), _) => Secret::new(p),
        (None, Some(saved)) => saved,
        (None, None) => return Err(error(&SyncError::EmptyPassphrase)),
    };
    // Checks the folder and the passphrase against a file that is there.
    file::read(&folder, secret.expose()).map_err(|e| error(&e))?;
    let dir = state_dir(&state)?;
    let changed_folder = folder_setting(&state).is_none_or(|f| f != folder);
    state.with_store(|s| s.set_secret(PASSPHRASE_ID, Some(&secret)))?;
    state.with_settings(|s| {
        s.set(
            "sync.folder",
            Some(toml_value(folder.display().to_string())),
        )?;
        s.set(
            "sync.interval_minutes",
            Some(toml_value(i64::from(interval_minutes.clamp(1, 24 * 60)))),
        )?;
        s.set("sync.on_start", Some(toml_value(on_start)))
    })?;
    if changed_folder {
        // A new folder starts like a new machine: nothing is known of the last sync.
        let _ = std::fs::remove_file(dir.join(STATE_FILE));
    }
    status_of(&state)
}

fn toml_value(v: impl Into<platypus_store::toml::Value>) -> platypus_store::toml::Value {
    v.into()
}

/// Turns the sync off: the folder, the passphrase and what was remembered are forgotten here.
/// The file in the folder stays.
#[tauri::command]
pub fn sync_disconnect(state: State<'_, AppState>) -> CommandResult<SyncStatus> {
    let dir = state_dir(&state)?;
    state.with_store(|s| s.set_secret(PASSPHRASE_ID, None))?;
    state.with_settings(|s| {
        for key in ["sync.folder", "sync.interval_minutes", "sync.on_start"] {
            s.set(key, None)?;
        }
        Ok(())
    })?;
    for name in [STATE_FILE, CONFLICTS_FILE] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    status_of(&state)
}

/// Forgets the conflicts the Sync page lists (their copies stay).
#[tauri::command]
pub fn sync_clear_conflicts(state: State<'_, AppState>) -> CommandResult<SyncStatus> {
    write_conflicts(&state_dir(&state)?, &[])?;
    status_of(&state)
}

/// Merges this machine's data with the sync file, both ways. Wrong passphrase, a file from a
/// newer app, a damaged file or a missing folder stop it before anything is changed.
#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> CommandResult<SyncReport> {
    let _running = RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let folder = folder_setting(&state)
        .ok_or_else(|| CommandError::new("sync", "no sync folder is chosen"))?;
    let secret = passphrase(&state)?.ok_or_else(|| error(&SyncError::EmptyPassphrase))?;
    let dir = state_dir(&state)?;
    let scripts = state
        .scripts
        .clone()
        .map_err(|e| CommandError::new("io", e))?;
    let report = state.with_store(|store| {
        let mut guard = lock(&state.settings);
        let Ok(settings) = &mut *guard else {
            return Ok(Err(SyncError::Invalid(
                "the settings could not be read".to_owned(),
            )));
        };
        let mut sync_state = SyncState::load(&dir.join(STATE_FILE));
        let result = run(
            store,
            settings,
            &Run {
                scripts: &scripts,
                folder: &folder,
                passphrase: secret.expose(),
                machine: &machine_name(),
                now: now_ms(),
            },
            &mut sync_state,
        )
        .and_then(|report| sync_state.save(&dir.join(STATE_FILE)).map(|()| report));
        Ok(result)
    })?;
    let report = report.map_err(|e| error(&e))?;
    if !report.conflicts.is_empty() {
        let mut all = report.conflicts.clone();
        all.extend(read_conflicts(&dir));
        all.truncate(CONFLICTS_KEPT);
        write_conflicts(&dir, &all)?;
    }
    Ok(report)
}

/// What a run needs besides the stores.
struct Run<'a> {
    scripts: &'a Path,
    folder: &'a Path,
    passphrase: &'a str,
    machine: &'a str,
    now: i64,
}

/// Everything this machine has, as values by item key.
fn current_values(
    store: &ConnectionStore,
    settings: &SettingsStore,
    scripts: &Path,
) -> (
    std::collections::BTreeMap<String, Value>,
    std::collections::BTreeMap<String, i64>,
) {
    let mut values = collect::setting_values(settings.all());
    values.extend(collect::workspace_values(store));
    let mut scopes = vec!["all".to_owned()];
    scopes.extend(
        store
            .projects()
            .iter()
            .filter(|p| p.shared.is_none())
            .map(|p| p.id.to_string()),
    );
    let (script_values, times) = collect::script_values(scripts, &scopes);
    values.extend(script_values);
    (values, times)
}

/// A readable name for a conflict's item.
fn describe(key: &str, items: &std::collections::BTreeMap<String, Item>) -> (String, String) {
    let (what, rest) = key.split_once('/').unwrap_or(("item", key));
    let name = match what {
        "script" => rest
            .split_once('/')
            .map_or(rest, |(_, path)| path)
            .to_owned(),
        "connection" | "project" | "profile" | "railfolder" => items
            .get(key)
            .and_then(|i| i.value.get("name"))
            .and_then(Value::as_str)
            .unwrap_or(rest)
            .to_owned(),
        _ => rest.to_owned(),
    };
    (what.to_owned(), name)
}

fn conflict_info(
    c: &Conflict,
    items: &std::collections::BTreeMap<String, Item>,
    at: i64,
) -> ConflictInfo {
    let (what, name) = describe(&c.key, items);
    ConflictInfo {
        at,
        what,
        name,
        kind: match c.kind {
            ConflictKind::BothEdited => "both_edited",
            ConflictKind::DeletedAndEdited => "deleted_and_edited",
        }
        .to_owned(),
        copy: c.copy.as_ref().map(|k| describe(k, items).1),
        machine: c.other_machine.clone(),
    }
}

/// Applies what the merge decided to this machine's stores.
fn apply(
    store: &mut ConnectionStore,
    settings: &mut SettingsStore,
    scripts: &Path,
    plan: Plan,
) -> Result<usize, SyncError> {
    let invalid = |e: platypus_store::StoreError| SyncError::Invalid(e.to_string());
    let applied = plan.batch.connections.len()
        + plan.batch.projects.len()
        + plan.batch.network_profiles.len()
        + plan.batch.project_folders.len()
        + plan.batch.remove_connections.len()
        + plan.batch.remove_projects.len()
        + plan.settings.len()
        + plan.scripts.len();
    store.apply_batch(plan.batch).map_err(invalid)?;
    for (key, value) in plan.settings {
        settings.set(&key, value).map_err(invalid)?;
    }
    collect::apply_scripts(scripts, &plan.scripts)?;
    Ok(applied)
}

/// One sync: read the file, merge, apply here, write the file when it changed, remember.
/// Nothing is changed unless the file reads.
fn run(
    store: &mut ConnectionStore,
    settings: &mut SettingsStore,
    run: &Run<'_>,
    state: &mut SyncState,
) -> Result<SyncReport, SyncError> {
    let remote = file::read(run.folder, run.passphrase)?;
    let remote_items = remote.map(|d| d.items).unwrap_or_default();
    let (values, times) = current_values(store, settings, run.scripts);
    let local = collect::local_items(&values, &times, &state.base, run.machine, run.now);
    let merged = merge(
        &local,
        &remote_items,
        &state.base,
        Context {
            now: run.now,
            machine: run.machine,
        },
    );
    let plan = collect::plan(&merged.apply, store);
    let skipped = plan.skipped.len();
    let applied = apply(store, settings, run.scripts, plan)?;
    let written = merged.items != remote_items;
    if written {
        file::write(
            run.folder,
            run.passphrase,
            &Document {
                format: DOCUMENT_FORMAT,
                machine: run.machine.to_owned(),
                written: run.now,
                items: merged.items.clone(),
            },
        )?;
    }
    state.base = merged
        .items
        .iter()
        .map(|(k, i)| (k.clone(), BaseEntry::of(i)))
        .collect();
    state.last_sync = Some(run.now);
    Ok(SyncReport {
        applied,
        written,
        skipped,
        conflicts: merged
            .conflicts
            .iter()
            .map(|c| conflict_info(c, &merged.items, run.now))
            .collect(),
        at: run.now,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use platypus_core::DriverKind;
    use platypus_store::{MemorySecrets, PasswordUpdate, Project, SavedConnection};

    use super::*;

    /// A machine: its connections file, settings and scripts folder.
    struct Machine {
        store: ConnectionStore,
        settings: SettingsStore,
        scripts: PathBuf,
        state: SyncState,
        name: &'static str,
    }

    fn machine(root: &Path, name: &'static str) -> Machine {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        Machine {
            store: ConnectionStore::open(
                dir.join("connections.toml"),
                Arc::new(MemorySecrets::default()),
            )
            .unwrap(),
            settings: SettingsStore::open(dir.join("settings.toml")).unwrap(),
            scripts: dir.join("scripts"),
            state: SyncState::default(),
            name,
        }
    }

    fn sync(m: &mut Machine, folder: &Path, now: i64) -> Result<SyncReport, SyncError> {
        run(
            &mut m.store,
            &mut m.settings,
            &Run {
                scripts: &m.scripts,
                folder,
                passphrase: "pass",
                machine: m.name,
                now,
            },
            &mut m.state,
        )
    }

    fn write_script(m: &Machine, path: &str, text: &str) {
        let p = m.scripts.join("all").join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn script(m: &Machine, path: &str) -> Option<String> {
        std::fs::read_to_string(m.scripts.join("all").join(path)).ok()
    }

    #[test]
    fn two_machines_converge_and_never_share_a_password() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("cloud");
        std::fs::create_dir(&folder).unwrap();
        let mut a = machine(root.path(), "A");
        let mut b = machine(root.path(), "B");
        let mut c = SavedConnection::new("Prod", DriverKind::Postgres);
        c.host = "db.example.com".to_owned();
        c.password_command = Some("echo CANARY_CMD".to_owned());
        c.save_password = true;
        let id = c.id;
        a.store.save(c, PasswordUpdate::Keep).unwrap();
        let mut p = Project::new("Team", "#112233");
        p.connections = vec![id];
        a.store.save_project(p).unwrap();
        a.settings.set("editor.vim", Some(true.into())).unwrap();
        a.settings.set("window.width", Some(900.into())).unwrap();
        write_script(&a, "reports/m.sql", "select 1");
        let first = sync(&mut a, &folder, 1_000).unwrap();
        assert!(first.written && first.applied == 0);
        let bytes = std::fs::read(folder.join(file::FILE_NAME)).unwrap();
        for leak in ["CANARY", "db.example.com", "select 1", "Prod"] {
            assert!(
                !bytes.windows(leak.len()).any(|w| w == leak.as_bytes()),
                "{leak}"
            );
        }
        let second = sync(&mut b, &folder, 2_000).unwrap();
        assert!(second.applied >= 4 && !second.written, "{second:?}");
        let got = b.store.get(id).unwrap();
        assert_eq!(
            (got.host.as_str(), got.password_command.clone()),
            ("db.example.com", None)
        );
        assert_eq!(b.store.projects()[0].connections, [id]);
        assert_eq!(b.settings.get::<bool>("editor.vim"), Some(true));
        assert_eq!(
            b.settings.get::<i64>("window.width"),
            None,
            "machine settings stay home"
        );
        assert_eq!(script(&b, "reports/m.sql").as_deref(), Some("select 1"));
        // Settled: another run on either machine changes nothing.
        let again = sync(&mut a, &folder, 3_000).unwrap();
        assert!(
            again.applied == 0 && !again.written && again.conflicts.is_empty(),
            "{again:?}"
        );
        let again = sync(&mut b, &folder, 4_000).unwrap();
        assert!(again.applied == 0 && !again.written, "{again:?}");
    }

    #[test]
    fn deletions_and_edits_travel_and_conflicts_keep_both() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("cloud");
        std::fs::create_dir(&folder).unwrap();
        let mut a = machine(root.path(), "A");
        let mut b = machine(root.path(), "B");
        write_script(&a, "keep.sql", "v1");
        write_script(&a, "gone.sql", "bye");
        write_script(&a, "both.sql", "base");
        sync(&mut a, &folder, 1_000).unwrap();
        sync(&mut b, &folder, 2_000).unwrap();
        // A deletes and edits; B edits the same script differently.
        std::fs::remove_file(a.scripts.join("all/gone.sql")).unwrap();
        write_script(&a, "keep.sql", "v2");
        write_script(&a, "both.sql", "from A");
        write_script(&b, "both.sql", "from B");
        sync(&mut a, &folder, 10_000).unwrap();
        let report = sync(&mut b, &folder, 20_000).unwrap();
        assert_eq!(script(&b, "gone.sql"), None);
        assert_eq!(script(&b, "keep.sql").as_deref(), Some("v2"));
        assert_eq!(report.conflicts.len(), 1, "{report:?}");
        assert_eq!(report.conflicts[0].name, "both.sql");
        assert_eq!(report.conflicts[0].machine, "A");
        let copy = report.conflicts[0].copy.clone().unwrap();
        assert!(
            copy.starts_with("both (conflict ") && copy.ends_with(").sql"),
            "{copy}"
        );
        // B wrote later, so its text keeps the name and A's goes to the copy.
        assert_eq!(script(&b, "both.sql").as_deref(), Some("from B"));
        assert_eq!(script(&b, &copy).as_deref(), Some("from A"));
        // A receives both and agrees.
        sync(&mut a, &folder, 30_000).unwrap();
        assert_eq!(script(&a, "both.sql").as_deref(), Some("from B"));
        assert_eq!(script(&a, &copy).as_deref(), Some("from A"));
        let settled = sync(&mut b, &folder, 40_000).unwrap();
        assert!(
            settled.applied == 0 && settled.conflicts.is_empty(),
            "{settled:?}"
        );
    }

    #[test]
    fn a_wrong_passphrase_or_a_missing_folder_changes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("cloud");
        std::fs::create_dir(&folder).unwrap();
        let mut a = machine(root.path(), "A");
        write_script(&a, "a.sql", "x");
        sync(&mut a, &folder, 1_000).unwrap();
        let before = std::fs::read(folder.join(file::FILE_NAME)).unwrap();
        let mut b = machine(root.path(), "B");
        write_script(&b, "b.sql", "mine");
        let err = run(
            &mut b.store,
            &mut b.settings,
            &Run {
                scripts: &b.scripts,
                folder: &folder,
                passphrase: "wrong",
                machine: "B",
                now: 2_000,
            },
            &mut b.state,
        )
        .unwrap_err();
        assert!(matches!(err, SyncError::WrongPassphrase));
        assert_eq!(std::fs::read(folder.join(file::FILE_NAME)).unwrap(), before);
        assert_eq!(script(&b, "b.sql").as_deref(), Some("mine"));
        assert_eq!(script(&b, "a.sql"), None);
        assert!(b.state.base.is_empty() && b.state.last_sync.is_none());
        let gone = root.path().join("unmounted");
        assert!(matches!(
            sync(&mut b, &gone, 3_000),
            Err(SyncError::Io { .. })
        ));
        assert!(!gone.exists());
    }
}
