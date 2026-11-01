//! End-to-end encrypted sync through a folder: settings, connections without passwords,
//! projects and scripts in one file (`platypus-sync.bin`) sealed with a sync passphrase that
//! is not the master password. The passphrase is kept in the keychain (or the vault) beside
//! the passwords; the folder, the interval and the run-at-start choice are settings under
//! `sync.` (which stay on the machine). What the last sync knew, to tell a change on both
//! sides from one on a side, is `sync-state.json` in the data folder.
//!
//! The page calls `sync_now` on demand, at start and every few minutes; the merge is in
//! `platypus-sync`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use platypus_core::Secret;
use platypus_mcp::profile::ProfileKind;
use platypus_store::{ConnectionStore, SettingsStore};
use platypus_sync::collect::{self, Plan, Withheld};
use platypus_sync::merge::{Conflict, ConflictKind, Context, Merged, merge};
use platypus_sync::model::{BaseEntry, DOCUMENT_FORMAT, Document, Item, SyncState};
use platypus_sync::{SyncError, file};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use uuid::Uuid;

use super::{AppState, lock};
use crate::error::{CommandError, CommandResult};

/// Where the sync passphrase was kept before each profile had its own (see
/// [`AppState::passphrase_id`]): one name for every profile on the computer, so that a portable
/// copy or a test profile could overwrite or remove the installed app's passphrase.
const SHARED_PASSPHRASE_ID: Uuid = Uuid::from_u128(0x504c_4154_5950_5553_5359_4e43_5041_5353);
const STATE_FILE: &str = "sync-state.json";
const CONFLICTS_FILE: &str = "sync-conflicts.json";
/// How many conflicts the Sync page remembers.
const CONFLICTS_KEPT: usize = 50;
const DEFAULT_INTERVAL: u32 = 15;

/// One run at a time: the interval, the start and a click can overlap.
static RUNNING: Mutex<()> = Mutex::new(());

pub(crate) fn error(e: &SyncError) -> CommandError {
    let kind = match e {
        SyncError::WrongPassphrase => "sync_passphrase",
        SyncError::PasswordNeeded => "password_needed",
        SyncError::EmptyPassphrase => "sync_passphrase_empty",
        SyncError::ShortPassphrase => "sync_passphrase_short",
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
    /// `both_edited`, `deleted_and_edited` or `first_sync`.
    pub kind: String,
    /// The name of the copy that keeps the other version, when there is one.
    pub copy: Option<String>,
    /// The computer that made the other version (for `first_sync`, the version kept).
    pub machine: String,
    /// What this computer had, when a first sync replaced a setting with the synced one.
    #[serde(default)]
    pub replaced: Option<String>,
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

impl AppState {
    /// Where this profile's sync passphrase is kept, beside the passwords: a name of the
    /// profile's own folder, as the agent policy's seal has.
    pub(super) fn passphrase_id(&self) -> CommandResult<Uuid> {
        let dir = self
            .vault_path
            .as_ref()
            .map_err(|e| CommandError::new("store", e.clone()))?;
        let dir = dir
            .parent()
            .ok_or_else(|| CommandError::new("io", "the data folder is not set"))?;
        Ok(platypus_mcp::profile::keychain_id(dir))
    }
}

/// The passphrase saved under `id`. Without one, a profile of `kind` whose sync has a folder
/// takes the one saved under [`SHARED_PASSPHRASE_ID`] by the versions before: the installed
/// app moves it to `id`, a portable copy copies it (it may be the installed app's), and a
/// separate profile (`PLATYPUS_DATA_DIR`, as the tests use) never reads it.
fn read_passphrase(
    store: &ConnectionStore,
    id: Uuid,
    kind: ProfileKind,
    has_folder: bool,
) -> platypus_store::Result<Option<Secret>> {
    if let Some(secret) = store.secret(id)? {
        return Ok(Some(secret));
    }
    if kind == ProfileKind::Separate || !has_folder {
        return Ok(None);
    }
    let Some(shared) = store.secret(SHARED_PASSPHRASE_ID)? else {
        return Ok(None);
    };
    store.set_secret(id, Some(&shared))?;
    if kind == ProfileKind::Installed {
        store.set_secret(SHARED_PASSPHRASE_ID, None)?;
    }
    Ok(Some(shared))
}

fn passphrase(state: &AppState) -> CommandResult<Option<Secret>> {
    let id = state.passphrase_id()?;
    let has_folder = folder_setting(state).is_some();
    state.with_store(|s| read_passphrase(s, id, ProfileKind::from_env(), has_folder))
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
    let typed = passphrase.filter(|p| !p.is_empty());
    let new = typed.is_some();
    let secret = match (typed, saved) {
        (Some(p), _) => Secret::new(p),
        (None, Some(saved)) => saved,
        (None, None) => return Err(error(&SyncError::EmptyPassphrase)),
    };
    // Checks the folder and the passphrase against a file that is there. A folder with none
    // starts a new sync, whose passphrase must be long enough; one that opens the file there
    // was chosen before, on another computer.
    let existing = file::read(&folder, secret.expose()).map_err(|e| error(&e))?;
    if new && existing.is_none() {
        platypus_sync::crypto::check_new_passphrase(secret.expose()).map_err(|e| error(&e))?;
    }
    let dir = state_dir(&state)?;
    let changed_folder = folder_setting(&state).is_none_or(|f| f != folder);
    let id = state.passphrase_id()?;
    state.with_store(|s| s.set_secret(id, Some(&secret)))?;
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
    let id = state.passphrase_id()?;
    state.with_store(|s| s.set_secret(id, None))?;
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

/// Everything this machine synchronises, as values by item key.
struct Current {
    values: BTreeMap<String, Value>,
    /// When the scripts' files changed.
    times: BTreeMap<String, i64>,
    /// What this machine has but keeps out of the sync for now (see `collect::Withheld`).
    withheld: Withheld,
}

fn current_values(store: &ConnectionStore, settings: &SettingsStore, scripts: &Path) -> Current {
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
    let found = collect::script_values(scripts, &scopes);
    values.extend(found.values);
    let mut withheld = collect::shared_keys(store);
    withheld.extend(found.withheld);
    Current {
        values,
        times: found.times,
        withheld,
    }
}

/// A readable name for a conflict's item.
fn describe(key: &str, items: &BTreeMap<String, Item>) -> (String, String) {
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

fn conflict_info(c: &Conflict, items: &BTreeMap<String, Item>, at: i64) -> ConflictInfo {
    let (what, name) = describe(&c.key, items);
    ConflictInfo {
        at,
        what,
        name,
        kind: match c.kind {
            ConflictKind::BothEdited => "both_edited",
            ConflictKind::DeletedAndEdited => "deleted_and_edited",
            ConflictKind::FirstSync => "first_sync",
        }
        .to_owned(),
        copy: c.copy.as_ref().map(|k| describe(k, items).1),
        machine: c.other_machine.clone(),
        replaced: c.replaced.as_ref().and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Bool(_) | Value::Number(_) => Some(v.to_string()),
            _ => None,
        }),
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
    let b = &plan.batch;
    let applied = b.connections.len()
        + b.projects.len()
        + b.network_profiles.len()
        + b.project_folders.len()
        + b.remove_connections.len()
        + b.remove_projects.len()
        + b.remove_network_profiles.len()
        + b.remove_project_folders.len()
        + plan.settings.len()
        + plan.scripts.len();
    store.apply_batch(plan.batch).map_err(invalid)?;
    for (key, value) in plan.settings {
        settings.set(&key, value).map_err(invalid)?;
    }
    collect::apply_scripts(scripts, &plan.scripts)?;
    Ok(applied)
}

/// How many times a run reads and merges again when another machine wrote the file while it
/// merged, before it writes anyway (what that write loses is caught at the next run, see
/// `next_base`).
const ATTEMPTS: usize = 3;

/// One sync: read the file, merge, apply here, write the file when it changed, remember.
/// Nothing is changed unless the file reads.
fn run(
    store: &mut ConnectionStore,
    settings: &mut SettingsStore,
    run: &Run<'_>,
    state: &mut SyncState,
) -> Result<SyncReport, SyncError> {
    run_racing(store, settings, run, state, &mut || {})
}

/// [`run`], with `merged` called after each merge and before the check that the file did not
/// change meanwhile: the tests write the file there, as another machine would.
fn run_racing(
    store: &mut ConnectionStore,
    settings: &mut SettingsStore,
    run: &Run<'_>,
    state: &mut SyncState,
    merged_hook: &mut dyn FnMut(),
) -> Result<SyncReport, SyncError> {
    let current = current_values(store, settings, run.scripts);
    let local = collect::local_items(
        &current.values,
        &current.times,
        &state.base,
        &current.withheld,
        run.machine,
        run.now,
    );
    let (remote_items, merged) = merge_settled(&local, run, state, &current.withheld, merged_hook)?;
    let plan = collect::plan(&merged.apply, store);
    let skipped = plan.skipped.len();
    let applied = apply(store, settings, run.scripts, plan)?;
    let written = merged.items != remote_items;
    let mut confirmed = None;
    if written {
        let stamp = file::write(
            run.folder,
            run.passphrase,
            &Document {
                format: DOCUMENT_FORMAT,
                machine: run.machine.to_owned(),
                written: run.now,
                items: merged.items.clone(),
            },
        )?;
        // Another machine may have written between the check and this write: what the file
        // holds now is what is synchronised, not what was meant to be.
        if file::stamp(run.folder)?.as_deref() != Some(stamp.as_str()) {
            // A file that does not read now confirms nothing: everything is tried again.
            confirmed = Some(
                file::read(run.folder, run.passphrase)
                    .ok()
                    .flatten()
                    .map(|d| d.items)
                    .unwrap_or_default(),
            );
        }
    }
    state.base = next_base(&state.base, &merged.items, confirmed.as_ref());
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

/// Reads the file and merges it with `local` until the file stays as it was read through the
/// merge (another machine may write it meanwhile), at most [`ATTEMPTS`] times. Returns the
/// file's items and the merge; what is withheld is never applied here.
fn merge_settled(
    local: &BTreeMap<String, Item>,
    run: &Run<'_>,
    state: &SyncState,
    withheld: &Withheld,
    merged_hook: &mut dyn FnMut(),
) -> Result<(BTreeMap<String, Item>, Merged), SyncError> {
    let mut attempt = 1;
    loop {
        let (remote, stamp) = file::read_stamped(run.folder, run.passphrase)?;
        let remote_items = remote.map(|d| d.items).unwrap_or_default();
        let mut merged = merge(
            local,
            &remote_items,
            &state.base,
            Context {
                now: run.now,
                machine: run.machine,
            },
        );
        merged.apply.retain(|c| !withheld.contains(c.key()));
        merged_hook();
        if attempt == ATTEMPTS || file::stamp(run.folder)? == stamp {
            return Ok((remote_items, merged));
        }
        attempt += 1;
    }
}

/// What this machine remembers after a run: the merged items, except those the file did not
/// end up holding (`confirmed`, when another machine's write replaced this one's), which keep
/// their old entry, so that a change made here is still a change at the next run instead of
/// being reverted by the other machine's older version.
fn next_base(
    old: &BTreeMap<String, BaseEntry>,
    merged: &BTreeMap<String, Item>,
    confirmed: Option<&BTreeMap<String, Item>>,
) -> BTreeMap<String, BaseEntry> {
    merged
        .iter()
        .filter_map(|(key, item)| {
            let in_file = confirmed.is_none_or(|file| {
                file.get(key)
                    .is_some_and(|f| f.fingerprint() == item.fingerprint())
            });
            let entry = if in_file {
                BaseEntry::after(old.get(key), item)
            } else {
                old.get(key)?.clone()
            };
            Some((key.clone(), entry))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use platypus_core::{DriverKind, SslMode};
    use platypus_store::{
        MemorySecrets, NetworkProfile, PasswordUpdate, Project, ProjectFolder, SavedConnection,
        SharedLink,
    };

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

    /// A profile's store with a passphrase under the name all profiles shared before.
    fn store_with_shared_passphrase(root: &Path) -> ConnectionStore {
        let store = ConnectionStore::open(
            root.join("connections.toml"),
            Arc::new(MemorySecrets::default()),
        )
        .unwrap();
        store
            .set_secret(SHARED_PASSPHRASE_ID, Some(&Secret::new("shared")))
            .unwrap();
        store
    }

    fn shared(store: &ConnectionStore) -> Option<String> {
        store
            .secret(SHARED_PASSPHRASE_ID)
            .unwrap()
            .map(|s| s.expose().to_owned())
    }

    #[test]
    fn the_installed_app_moves_the_shared_passphrase_to_its_own_name() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_shared_passphrase(dir.path());
        let id = Uuid::new_v4();
        let read = read_passphrase(&store, id, ProfileKind::Installed, true).unwrap();
        assert_eq!(read.unwrap().expose(), "shared");
        assert_eq!(store.secret(id).unwrap().unwrap().expose(), "shared");
        assert_eq!(shared(&store), None);
        // Read again, it is its own.
        let again = read_passphrase(&store, id, ProfileKind::Installed, true).unwrap();
        assert_eq!(again.unwrap().expose(), "shared");
    }

    #[test]
    fn a_portable_copy_copies_the_shared_passphrase_and_leaves_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_shared_passphrase(dir.path());
        let id = Uuid::new_v4();
        let read = read_passphrase(&store, id, ProfileKind::Portable, true).unwrap();
        assert_eq!(read.unwrap().expose(), "shared");
        assert_eq!(store.secret(id).unwrap().unwrap().expose(), "shared");
        assert_eq!(shared(&store).as_deref(), Some("shared"));
    }

    #[test]
    fn a_separate_profile_never_reads_the_shared_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_shared_passphrase(dir.path());
        let id = Uuid::new_v4();
        assert!(
            read_passphrase(&store, id, ProfileKind::Separate, true)
                .unwrap()
                .is_none()
        );
        assert!(store.secret(id).unwrap().is_none());
        assert_eq!(shared(&store).as_deref(), Some("shared"));
    }

    /// A profile whose sync is off takes nothing: a passphrase it removed when it turned the
    /// sync off does not come back from the shared name.
    #[test]
    fn a_profile_without_a_sync_folder_leaves_the_shared_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_shared_passphrase(dir.path());
        let id = Uuid::new_v4();
        for kind in [ProfileKind::Installed, ProfileKind::Portable] {
            assert!(read_passphrase(&store, id, kind, false).unwrap().is_none());
        }
        assert!(store.secret(id).unwrap().is_none());
        assert_eq!(shared(&store).as_deref(), Some("shared"));
    }

    #[test]
    fn a_profile_s_own_passphrase_wins_over_the_shared_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_shared_passphrase(dir.path());
        let id = Uuid::new_v4();
        store.set_secret(id, Some(&Secret::new("own"))).unwrap();
        let read = read_passphrase(&store, id, ProfileKind::Installed, true).unwrap();
        assert_eq!(read.unwrap().expose(), "own");
        assert_eq!(shared(&store).as_deref(), Some("shared"));
    }

    /// Two machines and the folder they share.
    fn pair(root: &Path) -> (Machine, Machine, PathBuf) {
        let folder = root.join("cloud");
        std::fs::create_dir(&folder).unwrap();
        (machine(root, "A"), machine(root, "B"), folder)
    }

    fn sync_racing(m: &mut Machine, folder: &Path, now: i64, hook: &mut dyn FnMut()) -> SyncReport {
        run_racing(
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
            hook,
        )
        .unwrap()
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
    fn sharing_a_project_is_not_a_deletion_on_the_other_machines() {
        let root = tempfile::tempdir().unwrap();
        let (mut a, mut b, folder) = pair(root.path());
        let p = Project::new("Team", "#112233");
        let pid = p.id;
        a.store.save_project(p).unwrap();
        let scope = a.scripts.join(pid.to_string());
        std::fs::create_dir_all(&scope).unwrap();
        std::fs::write(scope.join("q.sql"), "select 1").unwrap();
        sync(&mut a, &folder, 1_000).unwrap();
        sync(&mut b, &folder, 2_000).unwrap();
        assert_eq!(b.store.projects().len(), 1);
        let b_script = b.scripts.join(pid.to_string()).join("q.sql");
        assert!(b_script.exists());
        // A shares the project through a folder: it leaves the sync, but is not deleted.
        let mut shared = a.store.projects()[0].clone();
        shared.shared = Some(SharedLink {
            path: root.path().join("team"),
            share_user: false,
        });
        a.store
            .apply_batch(platypus_store::SyncBatch {
                projects: vec![shared],
                ..platypus_store::SyncBatch::default()
            })
            .unwrap();
        sync(&mut a, &folder, 3_000).unwrap();
        let report = sync(&mut b, &folder, 4_000).unwrap();
        assert_eq!(report.applied, 0, "{report:?}");
        assert_eq!(b.store.projects().len(), 1, "B keeps the project");
        assert!(b_script.exists(), "and its scripts");
        // B deletes it: the shared project on A stays, it lives in its folder now.
        b.store.delete_project(pid).unwrap();
        sync(&mut b, &folder, 5_000).unwrap();
        sync(&mut a, &folder, 6_000).unwrap();
        assert!(a.store.projects()[0].shared.is_some());
        let settled = sync(&mut a, &folder, 7_000).unwrap();
        assert!(settled.applied == 0 && !settled.written, "{settled:?}");
    }

    #[test]
    fn a_script_that_cannot_be_synchronised_is_not_deleted_elsewhere() {
        let root = tempfile::tempdir().unwrap();
        let (mut a, mut b, folder) = pair(root.path());
        write_script(&a, "big.sql", "select 1");
        sync(&mut a, &folder, 1_000).unwrap();
        sync(&mut b, &folder, 2_000).unwrap();
        // The file grows past the limit on A, and B's copy stays as it was.
        std::fs::File::create(a.scripts.join("all/big.sql"))
            .unwrap()
            .set_len(collect::MAX_SCRIPT_BYTES + 1)
            .unwrap();
        sync(&mut a, &folder, 3_000).unwrap();
        sync(&mut b, &folder, 4_000).unwrap();
        assert_eq!(script(&b, "big.sql").as_deref(), Some("select 1"));
        // An edit on B does not overwrite A's large file either.
        write_script(&b, "big.sql", "select 2");
        sync(&mut b, &folder, 5_000).unwrap();
        sync(&mut a, &folder, 6_000).unwrap();
        let len = std::fs::metadata(a.scripts.join("all/big.sql"))
            .unwrap()
            .len();
        assert_eq!(len, collect::MAX_SCRIPT_BYTES + 1);
    }

    #[test]
    fn a_new_machine_does_not_overwrite_what_the_others_agreed_on() {
        let root = tempfile::tempdir().unwrap();
        let (mut a, mut b, folder) = pair(root.path());
        a.settings
            .set("appearance.theme", Some("dark".into()))
            .unwrap();
        write_script(&a, "m.sql", "select 'team'");
        sync(&mut a, &folder, 1_000).unwrap();
        // B was used alone before; its first sync comes much later by the clock.
        b.settings
            .set("appearance.theme", Some("light".into()))
            .unwrap();
        b.settings.set("editor.vim", Some(true.into())).unwrap();
        write_script(&b, "m.sql", "select 'mine'");
        let report = sync(&mut b, &folder, 9_000_000).unwrap();
        assert_eq!(
            b.settings.get::<String>("appearance.theme").as_deref(),
            Some("dark")
        );
        assert_eq!(script(&b, "m.sql").as_deref(), Some("select 'team'"));
        let theme = report
            .conflicts
            .iter()
            .find(|c| c.what == "setting")
            .unwrap();
        assert_eq!(
            (
                theme.kind.as_str(),
                theme.replaced.as_deref(),
                theme.machine.as_str()
            ),
            ("first_sync", Some("light"), "A")
        );
        let copy = report
            .conflicts
            .iter()
            .find(|c| c.what == "script")
            .and_then(|c| c.copy.clone())
            .unwrap();
        assert_eq!(script(&b, &copy).as_deref(), Some("select 'mine'"));
        // What only B had travels; A's theme stays.
        sync(&mut a, &folder, 9_100_000).unwrap();
        assert_eq!(
            a.settings.get::<String>("appearance.theme").as_deref(),
            Some("dark")
        );
        assert_eq!(a.settings.get::<bool>("editor.vim"), Some(true));
        assert_eq!(script(&a, &copy).as_deref(), Some("select 'mine'"));
    }

    #[test]
    fn a_write_while_merging_is_merged_again_not_lost() {
        let root = tempfile::tempdir().unwrap();
        let (mut a, mut b, folder) = pair(root.path());
        write_script(&a, "x.sql", "x1");
        sync(&mut a, &folder, 1_000).unwrap();
        sync(&mut b, &folder, 2_000).unwrap();
        write_script(&a, "x.sql", "x2");
        write_script(&b, "y.sql", "from B");
        // B writes the file while A merges.
        let mut raced = false;
        let report = sync_racing(&mut a, &folder, 3_000, &mut || {
            if !raced {
                raced = true;
                sync(&mut b, &folder, 3_500).unwrap();
            }
        });
        assert!(raced && report.written);
        assert_eq!(script(&a, "y.sql").as_deref(), Some("from B"));
        sync(&mut b, &folder, 4_000).unwrap();
        assert_eq!(script(&b, "x.sql").as_deref(), Some("x2"));
        assert_eq!(script(&b, "y.sql").as_deref(), Some("from B"));
    }

    #[test]
    fn a_write_that_lands_after_ours_does_not_revert_our_change() {
        let root = tempfile::tempdir().unwrap();
        let (mut a, mut b, folder) = pair(root.path());
        write_script(&a, "x.sql", "x1");
        sync(&mut a, &folder, 1_000).unwrap();
        sync(&mut b, &folder, 2_000).unwrap();
        let file = folder.join(file::FILE_NAME);
        let before = std::fs::read(&file).unwrap();
        // A writes its change; B, which had read the file before, writes its own after it
        // (a cloud drive keeps the last upload): A's change is gone from the file.
        write_script(&a, "x.sql", "x2");
        sync(&mut a, &folder, 3_000).unwrap();
        let after_a = std::fs::read(&file).unwrap();
        std::fs::write(&file, &before).unwrap();
        write_script(&b, "y.sql", "from B");
        sync(&mut b, &folder, 3_500).unwrap();
        assert_ne!(std::fs::read(&file).unwrap(), after_a);
        // A's next run sees the file's x1 as stale and writes x2 again.
        let report = sync(&mut a, &folder, 4_000).unwrap();
        assert!(report.written && report.conflicts.is_empty(), "{report:?}");
        assert_eq!(script(&a, "x.sql").as_deref(), Some("x2"));
        assert_eq!(script(&a, "y.sql").as_deref(), Some("from B"));
        sync(&mut b, &folder, 5_000).unwrap();
        assert_eq!(script(&b, "x.sql").as_deref(), Some("x2"));
    }

    #[test]
    fn a_write_that_replaces_ours_is_not_remembered_as_synced() {
        let old = BTreeMap::from([(
            "setting/x".to_owned(),
            BaseEntry::of(&Item::live("a".into(), 1, "A")),
        )]);
        let merged = BTreeMap::from([
            ("setting/x".to_owned(), Item::live("b".into(), 2, "A")),
            ("setting/y".to_owned(), Item::live("y".into(), 2, "A")),
            ("setting/z".to_owned(), Item::live("z".into(), 2, "A")),
        ]);
        // The file now holds another machine's write: x as it was, y as ours, no z.
        let file = BTreeMap::from([
            ("setting/x".to_owned(), Item::live("a".into(), 1, "A")),
            ("setting/y".to_owned(), Item::live("y".into(), 2, "A")),
        ]);
        let base = next_base(&old, &merged, Some(&file));
        assert_eq!(
            base["setting/x"], old["setting/x"],
            "x is still to be synced"
        );
        assert_eq!(
            base["setting/y"].fingerprint,
            merged["setting/y"].fingerprint()
        );
        assert!(!base.contains_key("setting/z"));
        let all = next_base(&old, &merged, None);
        assert_eq!(all.len(), 3);
        assert_eq!(
            all["setting/x"].previous.as_ref().map(|p| p.modified),
            Some(1)
        );
    }

    #[test]
    fn removed_profiles_and_rail_folders_count_as_applied() {
        let root = tempfile::tempdir().unwrap();
        let (mut a, mut b, folder) = pair(root.path());
        let profile = NetworkProfile {
            id: Uuid::new_v4(),
            name: "VPN".to_owned(),
            ssl_mode: SslMode::Require,
            ssl_root_cert: None,
            ssl_client_cert: None,
            ssl_client_key: None,
            ssh: None,
        };
        let project = Project::new("Team", "#112233");
        let rail = ProjectFolder {
            id: Uuid::new_v4(),
            name: "Work".to_owned(),
            color: "#223344".to_owned(),
            projects: vec![project.id],
            collapsed: false,
        };
        a.store
            .apply_batch(platypus_store::SyncBatch {
                network_profiles: vec![profile.clone()],
                projects: vec![project],
                project_folders: vec![rail.clone()],
                ..platypus_store::SyncBatch::default()
            })
            .unwrap();
        sync(&mut a, &folder, 1_000).unwrap();
        sync(&mut b, &folder, 2_000).unwrap();
        assert_eq!(b.store.network_profiles().len(), 1);
        assert_eq!(b.store.project_folders().len(), 1);
        a.store.delete_network_profile(profile.id).unwrap();
        a.store.ungroup(rail.id).unwrap();
        sync(&mut a, &folder, 3_000).unwrap();
        let report = sync(&mut b, &folder, 4_000).unwrap();
        assert_eq!(report.applied, 2, "{report:?}");
        assert!(b.store.network_profiles().is_empty() && b.store.project_folders().is_empty());
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
