//! Shared team projects: a project linked to a plain folder (`platypus-project.toml` and
//! `scripts/`) that a team keeps in version control or on a shared drive. The folder holds no
//! credentials; each user's own are asked at the first connect and kept in their keychain.
//!
//! The app and the folder follow each other: what the app changes is written to the file
//! right away (`push`, run after every change of the connections file), and what changed in
//! the folder from outside is read when the window gets focus (`refresh_shared_projects`).
//! A file that does not read (a merge conflict left in it) is never overwritten. Removing the
//! project from the app never touches the folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use platypus_store::{ConnectionStore, Project, SharedLink, SyncBatch};
use platypus_sync::shared::{self, PROJECT_FILE, SCRIPTS_DIR};
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::projects::Workspace;
use super::{AppState, lock};
use crate::error::{CommandError, CommandResult};

type Seen = Mutex<HashMap<Uuid, String>>;

fn fail(e: &platypus_sync::SyncError) -> CommandError {
    CommandError::new("shared_project", e.to_string())
}

/// The scripts folder of a shared project, created on first use; the shared folder itself must
/// be there (a drive that is not mounted must not get a new folder made in its place).
pub(super) fn scripts_root(folder: &Path) -> CommandResult<PathBuf> {
    if !folder.is_dir() {
        return Err(CommandError::new(
            "shared_project",
            format!("the shared folder {} was not found", folder.display()),
        ));
    }
    let dir = folder.join(SCRIPTS_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|e| CommandError::new("io", format!("{}: {e}", dir.display())))?;
    Ok(dir)
}

fn link_of(project: &Project) -> Option<&SharedLink> {
    project.shared.as_ref()
}

/// Writes the files of the shared projects the app changed. Only when the file is as the app
/// last saw it: one changed from outside waits for `refresh_shared_projects`, so it is read
/// before anything of the app's overwrites it.
pub(super) fn push(store: &ConnectionStore, seen: &Seen) {
    for project in store.projects() {
        let Some(link) = link_of(project) else {
            continue;
        };
        let Ok(Some(text)) = shared::read_text(&link.path) else {
            continue;
        };
        if lock(seen).get(&project.id) != Some(&text) {
            continue;
        }
        let Ok(file) = shared::parse(&text) else {
            continue;
        };
        let mut built = shared::build(store, project, link.share_user);
        // Agent access is this computer's own: the file keeps what it gives.
        shared::keep_agent_access(&mut built, &file);
        if built == file {
            continue;
        }
        match shared::render(&built).and_then(|t| shared::write_text(&link.path, &t).map(|()| t)) {
            Ok(written) => {
                lock(seen).insert(project.id, written);
            }
            Err(e) => eprintln!("shared project {}: {e}", project.name),
        }
    }
}

/// A shared project's folder read by the app, or what stopped it.
#[derive(Debug, Serialize)]
pub struct SharedNotice {
    project: Uuid,
    name: String,
    /// `changed`, `missing` (the folder or file is gone) or `invalid` (it does not read).
    kind: &'static str,
    added: usize,
    removed: usize,
    changed: usize,
    /// Connections the file now sends elsewhere: their saved credentials were forgotten.
    retargeted: usize,
    /// Connections whose safety settings the file loosened: this computer's were kept.
    kept_stricter: usize,
    message: Option<String>,
}

impl SharedNotice {
    fn problem(project: &Project, kind: &'static str, message: String) -> Self {
        Self {
            project: project.id,
            name: project.name.clone(),
            kind,
            added: 0,
            removed: 0,
            changed: 0,
            retargeted: 0,
            kept_stricter: 0,
            message: Some(message),
        }
    }
}

/// Reads one shared project's folder: what changed from outside comes into the app, and what
/// the app changed goes out. `None` when nothing happened.
fn reconcile(store: &mut ConnectionStore, seen: &Seen, project: &Project) -> Option<SharedNotice> {
    let link = link_of(project)?.clone();
    let text = match shared::read_text(&link.path) {
        Ok(Some(text)) => text,
        Ok(None) => {
            return Some(SharedNotice::problem(
                project,
                "missing",
                format!("{PROJECT_FILE} is not in {}", link.path.display()),
            ));
        }
        Err(e) => return Some(SharedNotice::problem(project, "missing", e.to_string())),
    };
    let known = lock(seen).get(&project.id).cloned();
    if known.as_deref() == Some(text.as_str()) {
        return None;
    }
    let file = match shared::parse(&text) {
        Ok(file) => file,
        Err(e) => return Some(SharedNotice::problem(project, "invalid", e.to_string())),
    };
    let before = shared::build(store, project, link.share_user);
    let diff = shared::compare(&before, &file);
    let mut guarded = (0, 0);
    if !diff.is_empty() {
        let share_user = file.connections.iter().any(|c| c.user.is_some());
        let link = SharedLink { share_user, ..link };
        match shared::apply(store, Some(project.id), &file, link) {
            Ok(applied) => guarded = (applied.retargeted, applied.kept_stricter),
            Err(e) => return Some(SharedNotice::problem(project, "invalid", e.to_string())),
        }
    }
    lock(seen).insert(project.id, text);
    (!diff.is_empty()).then_some(SharedNotice {
        project: project.id,
        name: file.name,
        kind: "changed",
        added: diff.added,
        removed: diff.removed,
        changed: diff.changed,
        retargeted: guarded.0,
        kept_stricter: guarded.1,
        message: None,
    })
}

/// Reads every shared project's folder; the notices say what was picked up or what is wrong.
/// The page calls it when the window gets focus and after a manual refresh.
#[tauri::command]
pub fn refresh_shared_projects(state: State<'_, AppState>) -> CommandResult<Vec<SharedNotice>> {
    let mut notices = Vec::new();
    state.with_store(|s| {
        let projects: Vec<Project> = s
            .projects()
            .iter()
            .filter(|p| p.shared.is_some())
            .cloned()
            .collect();
        for project in &projects {
            notices.extend(reconcile(s, &state.shared_seen, project));
        }
        Ok(())
    })?;
    Ok(notices)
}

/// The scripts folder of the project as it is now, for copying out when it is shared.
fn local_scripts(state: &AppState, project: Uuid) -> CommandResult<PathBuf> {
    super::scripts::root(state, Some(project))
}

/// Writes `project` to `folder` and links it: `platypus-project.toml` (connections without
/// credentials, and user names only with `share_user`) and its scripts under `scripts/`.
/// The folder must exist and hold no shared project yet.
#[tauri::command]
pub fn share_project(
    state: State<'_, AppState>,
    project_id: Uuid,
    folder: PathBuf,
    share_user: bool,
) -> CommandResult<Workspace> {
    if !folder.is_dir() {
        return Err(CommandError::new(
            "shared_project",
            format!("{} is not a folder", folder.display()),
        ));
    }
    if folder.join(PROJECT_FILE).exists() {
        return Err(CommandError::new(
            "shared_project",
            format!("{} already holds a shared project", folder.display()),
        ));
    }
    let scripts = local_scripts(&state, project_id)?;
    let text = state.with_store(|s| {
        let project = s
            .projects()
            .iter()
            .find(|p| p.id == project_id)
            .ok_or(platypus_store::StoreError::NotFound(project_id))?;
        if project.shared.is_some() {
            return Err(platypus_store::StoreError::Invalid(
                "the project is shared already".to_owned(),
            ));
        }
        shared::render(&shared::build(s, project, share_user))
            .map_err(|e| platypus_store::StoreError::Invalid(e.to_string()))
    })?;
    shared::copy_scripts(&scripts, &folder).map_err(|e| fail(&e))?;
    shared::write_text(&folder, &text).map_err(|e| fail(&e))?;
    lock(&state.shared_seen).insert(project_id, text);
    state.with_store(|s| {
        let mut project = s
            .projects()
            .iter()
            .find(|p| p.id == project_id)
            .cloned()
            .ok_or(platypus_store::StoreError::NotFound(project_id))?;
        project.shared = Some(SharedLink {
            path: folder,
            share_user,
        });
        s.apply_batch(SyncBatch {
            projects: vec![project],
            ..SyncBatch::default()
        })?;
        Ok(Workspace::of(s))
    })
}

/// What opening a shared folder made.
#[derive(Debug, Serialize)]
pub struct OpenedShared {
    workspace: Workspace,
    project: Uuid,
}

/// Opens the shared project in `folder` as a project of the app, linked to it. Connections
/// come without credentials: the first connect of each asks for them.
#[tauri::command]
pub fn open_shared_project(
    state: State<'_, AppState>,
    folder: PathBuf,
) -> CommandResult<OpenedShared> {
    let text = shared::read_text(&folder)
        .map_err(|e| fail(&e))?
        .ok_or_else(|| {
            CommandError::new(
                "shared_project",
                format!("{PROJECT_FILE} is not in {}", folder.display()),
            )
        })?;
    let file = shared::parse(&text).map_err(|e| fail(&e))?;
    let canonical = std::fs::canonicalize(&folder).unwrap_or_else(|_| folder.clone());
    let opened = state.with_store(|s| {
        let linked = s.projects().iter().any(|p| {
            link_of(p).is_some_and(|l| {
                std::fs::canonicalize(&l.path).unwrap_or_else(|_| l.path.clone()) == canonical
            })
        });
        if linked {
            return Err(platypus_store::StoreError::Invalid(
                "this folder is already open as a project".to_owned(),
            ));
        }
        let link = SharedLink {
            path: folder,
            share_user: file.connections.iter().any(|c| c.user.is_some()),
        };
        let id = shared::apply(s, None, &file, link)
            .map_err(|e| platypus_store::StoreError::Invalid(e.to_string()))?
            .id;
        Ok(OpenedShared {
            workspace: Workspace::of(s),
            project: id,
        })
    })?;
    lock(&state.shared_seen).insert(opened.project, text);
    Ok(opened)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use platypus_core::DriverKind;
    use platypus_store::{MemorySecrets, PasswordUpdate, SavedConnection};

    use super::*;

    fn store(dir: &Path) -> ConnectionStore {
        ConnectionStore::open(
            dir.join("connections.toml"),
            Arc::new(MemorySecrets::default()),
        )
        .unwrap()
    }

    /// A shared project linked to `team`, written once, as the app leaves it after sharing.
    fn linked(dir: &Path) -> (ConnectionStore, Seen, Uuid, PathBuf) {
        let team = dir.join("team");
        std::fs::create_dir_all(&team).unwrap();
        let mut s = store(&dir.join("app"));
        let c = SavedConnection::new("Local", DriverKind::Sqlite);
        let cid = c.id;
        s.save(c, PasswordUpdate::Keep).unwrap();
        let mut p = Project::new("Team", "#112233");
        p.connections = vec![cid];
        let pid = p.id;
        s.save_project(p).unwrap();
        let mut p = s.projects()[0].clone();
        p.shared = Some(SharedLink {
            path: team.clone(),
            share_user: false,
        });
        s.apply_batch(SyncBatch {
            projects: vec![p.clone()],
            ..SyncBatch::default()
        })
        .unwrap();
        let text = shared::render(&shared::build(&s, &p, false)).unwrap();
        shared::write_text(&team, &text).unwrap();
        let seen = Mutex::new(HashMap::from([(pid, text)]));
        (s, seen, pid, team)
    }

    /// What agents may do here stays here: turning them off writes nothing to the team's file.
    #[test]
    fn agent_access_is_not_written_to_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, seen, _pid, team) = linked(dir.path());
        let before = std::fs::read_to_string(team.join(PROJECT_FILE)).unwrap();
        let mut c = s.list()[0].clone();
        c.agent_access = platypus_store::AgentAccess::Off;
        s.save(c, PasswordUpdate::Keep).unwrap();
        push(&s, &seen);
        let after = std::fs::read_to_string(team.join(PROJECT_FILE)).unwrap();
        assert_eq!(after, before);
    }

    #[test]
    fn app_changes_are_written_to_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, seen, pid, team) = linked(dir.path());
        let mut p = s.projects()[0].clone();
        p.name = "Renamed".to_owned();
        s.save_project(p).unwrap();
        push(&s, &seen);
        let text = std::fs::read_to_string(team.join(PROJECT_FILE)).unwrap();
        assert!(text.contains("Renamed"), "{text}");
        assert_eq!(lock(&seen).get(&pid), Some(&text));
        // Nothing to write: the file is not touched again.
        let before = std::fs::metadata(team.join(PROJECT_FILE))
            .unwrap()
            .modified()
            .unwrap();
        push(&s, &seen);
        assert_eq!(
            std::fs::metadata(team.join(PROJECT_FILE))
                .unwrap()
                .modified()
                .unwrap(),
            before
        );
    }

    #[test]
    fn an_outside_change_is_read_and_not_overwritten_by_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, seen, pid, team) = linked(dir.path());
        let path = team.join(PROJECT_FILE);
        let edited = std::fs::read_to_string(&path)
            .unwrap()
            .replace("Team", "Edited outside");
        std::fs::write(&path, &edited).unwrap();
        // The app changes something too: its push waits for the read.
        let mut p = s.projects()[0].clone();
        p.color = "#445566".to_owned();
        s.save_project(p).unwrap();
        push(&s, &seen);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        let project = s.projects()[0].clone();
        let notice = reconcile(&mut s, &seen, &project).unwrap();
        assert_eq!(
            (notice.kind, notice.name.as_str()),
            ("changed", "Edited outside")
        );
        assert_eq!(s.projects()[0].name, "Edited outside");
        assert_eq!(lock(&seen).get(&pid), Some(&edited));
        // Nothing changed since: no notice.
        let project = s.projects()[0].clone();
        assert!(reconcile(&mut s, &seen, &project).is_none());
    }

    #[test]
    fn an_outside_edit_that_moves_a_connection_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, seen, _pid, team) = linked(dir.path());
        let path = team.join(PROJECT_FILE);
        let mut file = shared::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        file.connections[0].database = "/elsewhere/other.db".to_owned();
        file.connections[0].agent_access = platypus_store::AgentAccess::ReadWrite;
        std::fs::write(&path, shared::render(&file).unwrap()).unwrap();
        let project = s.projects()[0].clone();
        let notice = reconcile(&mut s, &seen, &project).unwrap();
        assert_eq!(
            (notice.kind, notice.retargeted, notice.kept_stricter),
            ("changed", 1, 1)
        );
        let c = s.get(file.connections[0].id).unwrap();
        assert_eq!(c.database, "/elsewhere/other.db");
        assert_eq!(c.agent_access, platypus_store::AgentAccess::ReadOnly);
    }

    #[test]
    fn a_file_that_does_not_read_is_reported_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, seen, _pid, team) = linked(dir.path());
        let path = team.join(PROJECT_FILE);
        let broken = "<<<<<<< HEAD\nversion = 1\n=======\n>>>>>>> theirs\n";
        std::fs::write(&path, broken).unwrap();
        let project = s.projects()[0].clone();
        let notice = reconcile(&mut s, &seen, &project).unwrap();
        assert_eq!(notice.kind, "invalid");
        push(&s, &seen);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
        // A folder that is gone is reported, and nothing is created in its place.
        std::fs::remove_dir_all(&team).unwrap();
        let notice = reconcile(&mut s, &seen, &project).unwrap();
        assert_eq!(notice.kind, "missing");
        assert!(!team.exists());
        assert!(scripts_root(&team).is_err());
    }
}
