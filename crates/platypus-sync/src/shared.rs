//! A project shared through a plain folder, meant for a team's Git repository or a shared
//! drive and for reviewing in a pull request:
//!
//! ```text
//! platypus-project.toml   name, color, connection folders, connections without credentials
//! scripts/                the project's scripts (.sql) and .links.json
//! ```
//!
//! Each user keeps their own credentials in their keychain: the first connect asks for them.

use std::collections::BTreeMap;
use std::path::Path;

use platypus_store::{
    AgentAccess, ConnectionFolder, ConnectionStore, LINKS_FILE, Project, SavedSsh, SharedLink,
    SyncBatch, initials_of,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::portable::{ShareConnection, guard};
use crate::{Result, SyncError};

/// The project's file in the shared folder.
pub const PROJECT_FILE: &str = "platypus-project.toml";
/// The folder of the scripts in the shared folder.
pub const SCRIPTS_DIR: &str = "scripts";
/// The newest layout this build reads and writes.
pub const FORMAT_VERSION: u32 = 1;

const HEADER: &str = "# A PlatypusDB project shared with a team.\n\
# It holds no passwords, keys or key paths: everyone enters their own on first connect.\n\n";

/// The content of `platypus-project.toml`. Plain values first: TOML writes tables after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedProject {
    pub version: u32,
    pub id: Uuid,
    pub name: String,
    pub color: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<ConnectionFolder>,
    #[serde(default)]
    pub connections: Vec<ShareConnection>,
}

/// The shared form of `project`: its connections in order, each with its folder, a network
/// profile written into the connection (profiles are not shared), user names only when asked.
#[must_use]
pub fn build(store: &ConnectionStore, project: &Project, share_user: bool) -> SharedProject {
    let connections = project
        .connections
        .iter()
        .filter_map(|id| store.get(*id))
        .map(|c| {
            let mut shared = ShareConnection::from_saved(&store.effective(c), share_user);
            shared.network_profile = None;
            shared.folder = project.placement.get(&c.id).copied();
            shared
        })
        .collect();
    SharedProject {
        version: FORMAT_VERSION,
        id: project.id,
        name: project.name.clone(),
        color: project.color.clone(),
        folders: project.folders.clone(),
        connections,
    }
}

/// Agent access is each computer's own: `built`, this computer's form of the project, keeps
/// the access `file` gives the connections it lists already, so that a choice made here (a
/// connection new here starts with agents kept out, see [`apply`]) does not reach the team.
pub fn keep_agent_access(built: &mut SharedProject, file: &SharedProject) {
    for c in &mut built.connections {
        if let Some(theirs) = file.connections.iter().find(|f| f.id == c.id) {
            c.agent_access = theirs.agent_access;
        }
    }
}

/// The text of the project file.
pub fn render(project: &SharedProject) -> Result<String> {
    let body = toml::to_string_pretty(project).map_err(|e| SyncError::Invalid(e.to_string()))?;
    Ok(format!("{HEADER}{body}"))
}

/// Reads the text of a project file. A layout from a newer app is refused, not guessed at.
pub fn parse(text: &str) -> Result<SharedProject> {
    #[derive(Deserialize)]
    struct Version {
        version: u32,
    }
    let version: Version = toml::from_str(text)
        .map_err(|e| SyncError::Invalid(format!("{PROJECT_FILE} is not valid: {e}")))?;
    if version.version > FORMAT_VERSION {
        return Err(SyncError::Invalid(format!(
            "{PROJECT_FILE} was written by a newer PlatypusDB (layout {}): update the app",
            version.version
        )));
    }
    toml::from_str(text)
        .map_err(|e| SyncError::Invalid(format!("{PROJECT_FILE} is not valid: {e}")))
}

fn io(path: &Path, e: &std::io::Error) -> SyncError {
    SyncError::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    }
}

/// The text of the project file in `dir`; `None` when the folder holds none. A folder that is
/// missing is an error (a drive not mounted).
pub fn read_text(dir: &Path) -> Result<Option<String>> {
    if !dir.is_dir() {
        return Err(SyncError::Io {
            path: dir.display().to_string(),
            message: "the shared folder was not found".to_owned(),
        });
    }
    let path = dir.join(PROJECT_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(&path, &e)),
    }
}

/// Writes the project file in `dir` atomically.
pub fn write_text(dir: &Path, text: &str) -> Result<()> {
    crate::file::write_atomic(&dir.join(PROJECT_FILE), text.as_bytes())
}

/// Copies the scripts of `from` (a project's local folder) into `dir/scripts`, except those
/// that are already there, and returns how many were copied. Only `.sql` files and the links.
pub fn copy_scripts(from: &Path, dir: &Path) -> Result<usize> {
    let to = dir.join(SCRIPTS_DIR);
    std::fs::create_dir_all(&to).map_err(|e| io(&to, &e))?;
    let mut copied = 0;
    for entry in platypus_store::list_scripts(from)
        .into_iter()
        .filter(|e| !e.folder)
    {
        let (src, dest) = (from.join(&entry.path), to.join(&entry.path));
        if dest.exists() {
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(parent, &e))?;
        }
        std::fs::copy(&src, &dest).map_err(|e| io(&src, &e))?;
        copied += 1;
    }
    let links = platypus_store::read_script_links(from);
    if !links.is_empty() && !to.join(LINKS_FILE).exists() {
        let text =
            serde_json::to_string_pretty(&links).map_err(|e| SyncError::Invalid(e.to_string()))?;
        std::fs::write(to.join(LINKS_FILE), text).map_err(|e| io(&to, &e))?;
    }
    Ok(copied)
}

/// What a read of the project file changed in the app.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    /// The name, color or folders changed.
    pub project: bool,
}

impl Changes {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// What differs between two versions of a project, for the notice shown after a read.
#[must_use]
pub fn compare(before: &SharedProject, after: &SharedProject) -> Changes {
    let old: BTreeMap<Uuid, &ShareConnection> =
        before.connections.iter().map(|c| (c.id, c)).collect();
    let new: BTreeMap<Uuid, &ShareConnection> =
        after.connections.iter().map(|c| (c.id, c)).collect();
    Changes {
        added: new.keys().filter(|id| !old.contains_key(id)).count(),
        removed: old.keys().filter(|id| !new.contains_key(id)).count(),
        changed: new
            .iter()
            .filter(|(id, c)| old.get(id).is_some_and(|o| o != *c))
            .count(),
        project: (&before.name, &before.color, &before.folders)
            != (&after.name, &after.color, &after.folders)
            || before
                .connections
                .iter()
                .map(|c| c.id)
                .ne(after.connections.iter().map(|c| c.id)),
    }
}

/// What [`apply`] made of a project file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    /// The project's id.
    pub id: Uuid,
    /// Connections the file sends somewhere else: the credentials this machine kept for them
    /// were forgotten, so their next connect asks again (see [`guard`]).
    pub retargeted: usize,
    /// Connections whose safety settings the file loosened: this machine's were kept.
    pub kept_stricter: usize,
}

/// Makes the project in `store` what `shared` says, linked to `link`. `local_id` is the
/// project to update; `None` makes a new one, with the file's id unless the app has a project
/// with it. Connections are created or updated by id and keep their private settings; the
/// ones the file no longer lists leave the project but stay saved.
///
/// Anyone who can write to the folder can edit the file, so an update to a connection saved
/// here is checked first ([`guard`]): one that now goes to another server, database or tunnel
/// loses the password, SSH secrets and password command this machine kept for it (forgotten
/// before anything is written, so a keychain that refuses stops the update), and its safety
/// settings are never loosened by the file.
pub fn apply(
    store: &mut ConnectionStore,
    local_id: Option<Uuid>,
    shared: &SharedProject,
    link: SharedLink,
) -> Result<Applied> {
    let id = local_id.unwrap_or_else(|| {
        if store.projects().iter().any(|p| p.id == shared.id) {
            Uuid::new_v4()
        } else {
            shared.id
        }
    });
    let mut project = store
        .projects()
        .iter()
        .find(|p| p.id == id)
        .cloned()
        .unwrap_or_else(|| Project::new(&shared.name, &shared.color));
    project.id = id;
    project.name.clone_from(&shared.name);
    project.color.clone_from(&shared.color);
    project.initials = initials_of(&shared.name);
    project.folders.clone_from(&shared.folders);
    project.connections = shared.connections.iter().map(|c| c.id).collect();
    project.placement = shared
        .connections
        .iter()
        .filter_map(|c| c.folder.map(|f| (c.id, f)))
        .collect();
    project.shared = Some(link);
    let mut applied = Applied {
        id,
        retargeted: 0,
        kept_stricter: 0,
    };
    let mut forget = Vec::new();
    let mut connections = Vec::new();
    for incoming in &shared.connections {
        let existing = store.get(incoming.id);
        let mut saved = incoming.clone().into_saved(existing);
        if let Some(existing) = existing {
            let guarded = guard(&store.effective(existing), &mut saved);
            if guarded.retargeted {
                applied.retargeted += 1;
                // The connection's own tunnel only: a network profile's secrets serve others.
                forget.push(existing.id);
                forget.extend(
                    existing
                        .ssh
                        .iter()
                        .flat_map(SavedSsh::chain)
                        .map(|s| s.secret_id),
                );
                forget.extend(
                    saved
                        .ssh
                        .iter()
                        .flat_map(SavedSsh::chain)
                        .map(|s| s.secret_id),
                );
            }
            applied.kept_stricter += usize::from(guarded.kept_stricter);
        } else {
            // What agents may do on this computer is this computer's say: a connection new
            // here starts with agents kept out, whatever the file grants, until the user
            // changes it. Read-only, production and restrictions only make it stricter.
            saved.agent_access = AgentAccess::Off;
        }
        connections.push(saved);
    }
    let invalid = |e: platypus_store::StoreError| SyncError::Invalid(e.to_string());
    for secret_id in forget {
        store.set_secret(secret_id, None).map_err(invalid)?;
    }
    store
        .apply_batch(SyncBatch {
            connections,
            projects: vec![project],
            ..SyncBatch::default()
        })
        .map_err(invalid)?;
    Ok(applied)
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

    /// A project with two connections, one in a folder, full of things that must not leave.
    fn sample(s: &mut ConnectionStore) -> Uuid {
        let mut a = SavedConnection::new("Prod", DriverKind::Postgres);
        a.host = "db.example.com".to_owned();
        a.user = "alice".to_owned();
        a.password_command = Some("echo CANARY_CMD".to_owned());
        a.options
            .insert("password".to_owned(), "CANARY_OPT".to_owned());
        let b = SavedConnection::new("Local", DriverKind::Sqlite);
        let (aid, bid) = (a.id, b.id);
        s.save(a, PasswordUpdate::Keep).unwrap();
        s.save(b, PasswordUpdate::Keep).unwrap();
        let mut p = Project::new("Team", "#112233");
        p.connections = vec![aid, bid];
        let pid = p.id;
        s.save_project(p).unwrap();
        let folder = s.create_folder(pid, "Databases", None).unwrap();
        s.place_connection(pid, aid, Some(folder)).unwrap();
        pid
    }

    #[test]
    fn the_project_file_holds_no_credential_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let pid = sample(&mut s);
        let project = s.projects().iter().find(|p| p.id == pid).unwrap().clone();
        for share_user in [false, true] {
            let shared = build(&s, &project, share_user);
            let text = render(&shared).unwrap();
            for secret in [
                "CANARY",
                "password_command",
                "pgpass",
                "secret_id",
                "key_path",
                "ssl_client_key",
            ] {
                assert!(!text.contains(secret), "{secret} in {text}");
            }
            assert_eq!(text.contains("alice"), share_user, "{text}");
            assert!(text.contains("db.example.com") && text.contains("Databases"));
            assert_eq!(parse(&text).unwrap(), shared);
        }
    }

    #[test]
    fn opening_creates_the_project_and_a_second_read_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut origin = store(&dir.path().join("a"));
        let pid = sample(&mut origin);
        let project = origin
            .projects()
            .iter()
            .find(|p| p.id == pid)
            .unwrap()
            .clone();
        let mut shared = build(&origin, &project, true);
        // The file would let agents write: here they start kept out of every connection.
        shared.connections[0].agent_access = AgentAccess::ReadWrite;
        let mut mine = store(&dir.path().join("b"));
        let link = SharedLink {
            path: dir.path().join("team"),
            share_user: true,
        };
        let id = apply(&mut mine, None, &shared, link.clone()).unwrap().id;
        assert_eq!(id, pid, "the file's id is used when free");
        let opened = mine.projects().iter().find(|p| p.id == id).unwrap().clone();
        assert_eq!(opened.shared, Some(link.clone()));
        assert_eq!(opened.connections.len(), 2);
        // Asked for on first connect, kept in the keychain after.
        let prod = mine.get(shared.connections[0].id).unwrap();
        assert!(prod.save_password && prod.password_command.is_none());
        assert!(!mine.has_password(prod.id).unwrap());
        for c in &shared.connections {
            assert_eq!(mine.get(c.id).unwrap().agent_access, AgentAccess::Off);
        }
        // What the app builds from it is what was read: no write-back, no churn in Git (the
        // file keeps the agent access it gives).
        let mut built = build(&mine, &opened, true);
        keep_agent_access(&mut built, &shared);
        assert_eq!(built, shared);
        // An update leaves them out still: the file never loosens this computer's choice.
        apply(&mut mine, Some(id), &shared, link.clone()).unwrap();
        assert_eq!(
            mine.get(shared.connections[0].id).unwrap().agent_access,
            AgentAccess::Off
        );
        let writes = mine.revision();
        apply(&mut mine, Some(id), &shared, link).unwrap();
        assert_eq!(mine.projects().len(), 1);
        assert!(mine.revision() > writes, "applying writes the file once");
    }

    #[test]
    fn an_update_from_the_file_keeps_the_private_settings_and_drops_removed_connections() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let pid = sample(&mut s);
        let project = s.projects().iter().find(|p| p.id == pid).unwrap().clone();
        let link = SharedLink {
            path: dir.path().join("team"),
            share_user: false,
        };
        let mut shared = build(&s, &project, false);
        shared.name = "Team renamed".to_owned();
        let removed = shared.connections.pop().unwrap();
        shared.connections[0].host = "new.example.com".to_owned();
        let prod = shared.connections[0].id;
        let change = compare(&build(&s, &project, false), &shared);
        assert_eq!(
            (change.added, change.removed, change.changed, change.project),
            (0, 1, 1, true)
        );
        apply(&mut s, Some(pid), &shared, link).unwrap();
        let p = s.projects().iter().find(|p| p.id == pid).unwrap();
        assert_eq!((p.name.as_str(), p.connections.len()), ("Team renamed", 1));
        assert!(s.get(removed.id).is_some(), "the connection stays saved");
        let prod = s.get(prod).unwrap();
        assert_eq!(prod.host, "new.example.com");
        assert_eq!(prod.user, "alice", "no shared user: this machine's stays");
        assert_eq!(prod.options["password"], "CANARY_OPT");
    }

    #[test]
    fn an_edit_that_sends_a_connection_elsewhere_forgets_its_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let pid = sample(&mut s);
        let project = s.projects().iter().find(|p| p.id == pid).unwrap().clone();
        let prod = project.connections[0];
        let mut c = s.get(prod).unwrap().clone();
        c.ssh = Some(SavedSsh {
            host: "bastion".to_owned(),
            port: 22,
            user: "alice".to_owned(),
            auth: platypus_store::SshAuthKind::Password,
            key_path: None,
            secret_id: Uuid::new_v4(),
            jump: None,
        });
        c.agent_access = platypus_store::AgentAccess::Off;
        c.read_only = true;
        let ssh_secret = c.ssh.as_ref().unwrap().secret_id;
        s.save(
            c,
            PasswordUpdate::Set(platypus_core::Secret::new("CANARY_PW")),
        )
        .unwrap();
        s.set_secret(ssh_secret, Some(&platypus_core::Secret::new("CANARY_SSH")))
            .unwrap();
        let link = SharedLink {
            path: dir.path().join("team"),
            share_user: false,
        };
        let project = s.projects().iter().find(|p| p.id == pid).unwrap().clone();
        let original = build(&s, &project, false);

        // A rename, and a loosening of the safety settings: the credentials stay, the
        // stricter settings too.
        let mut renamed = original.clone();
        renamed.connections[0].name = "Production".to_owned();
        renamed.connections[0].agent_access = platypus_store::AgentAccess::ReadWrite;
        renamed.connections[0].read_only = false;
        let applied = apply(&mut s, Some(pid), &renamed, link.clone()).unwrap();
        assert_eq!((applied.retargeted, applied.kept_stricter), (0, 1));
        let saved = s.get(prod).unwrap();
        assert_eq!(saved.name, "Production");
        assert_eq!(saved.agent_access, platypus_store::AgentAccess::Off);
        assert!(saved.read_only);
        assert_eq!(saved.password_command.as_deref(), Some("echo CANARY_CMD"));
        assert!(s.has_password(prod).unwrap());
        assert!(s.secret(ssh_secret).unwrap().is_some());

        // The server, or the tunnel, goes elsewhere: nothing this machine kept for it follows.
        for change in [
            &(|c: &mut ShareConnection| c.host = "evil.example.com".to_owned())
                as &dyn Fn(&mut ShareConnection),
            &|c| c.ssh.as_mut().unwrap().host = "evil-bastion".to_owned(),
        ] {
            s.set_password(prod, &platypus_core::Secret::new("CANARY_PW"))
                .unwrap();
            s.set_secret(ssh_secret, Some(&platypus_core::Secret::new("CANARY_SSH")))
                .unwrap();
            let project = s.projects().iter().find(|p| p.id == pid).unwrap().clone();
            let mut moved = build(&s, &project, false);
            change(&mut moved.connections[0]);
            let applied = apply(&mut s, Some(pid), &moved, link.clone()).unwrap();
            assert_eq!(applied.retargeted, 1);
            let saved = s.get(prod).unwrap();
            assert!(saved.password_command.is_none());
            assert!(saved.save_password, "asked again at the next connect");
            assert!(!s.has_password(prod).unwrap());
            assert!(s.secret(ssh_secret).unwrap().is_none());
        }
    }

    #[test]
    fn a_hand_written_file_with_a_secret_in_it_is_taken_in_without_the_secret() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let id = Uuid::new_v4();
        let text = format!(
            "version = 1\nid = \"{}\"\nname = \"Hand made\"\ncolor = \"#2f9e6e\"\n\n[[connections]]\nid = \"{id}\"\nname = \"Sales\"\ndriver = \"sqlite\"\ndatabase = 'sales.db'\n\n[connections.options]\nwarehouse = \"WH\"\npassword = \"CANARY\"\nconnection_string = \"Server=h;PWD=CANARY\"\n",
            Uuid::new_v4()
        );
        let file = parse(&text).unwrap();
        let link = SharedLink {
            path: dir.path().join("team"),
            share_user: false,
        };
        apply(&mut s, None, &file, link).unwrap();
        let saved = s.get(id).unwrap();
        assert_eq!(
            saved.options.get("warehouse").map(String::as_str),
            Some("WH")
        );
        assert!(!saved.options.contains_key("password"));
        assert_eq!(saved.options["connection_string"], "Server=h");
        let stored = std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        assert!(!stored.contains("CANARY"), "{stored}");
    }

    #[test]
    fn bad_and_newer_files_are_refused() {
        assert!(parse("this is = not [valid").is_err());
        assert!(parse("name = \"x\"\n").is_err(), "no version");
        let err = parse("version = 99\nid = \"x\"\n").unwrap_err().to_string();
        assert!(err.contains("newer"), "{err}");
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_text(dir.path()).unwrap(), None);
        assert!(read_text(&dir.path().join("gone")).is_err());
    }

    #[test]
    fn scripts_are_copied_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("from"), dir.path().join("to"));
        std::fs::create_dir_all(from.join("reports")).unwrap();
        std::fs::write(from.join("reports/a.sql"), "select 1").unwrap();
        std::fs::write(from.join("b.sql"), "select 2").unwrap();
        std::fs::write(from.join("notes.txt"), "no").unwrap();
        let c = Uuid::new_v4();
        std::fs::write(from.join(LINKS_FILE), format!("{{\"b.sql\": \"{c}\"}}")).unwrap();
        std::fs::create_dir_all(to.join(SCRIPTS_DIR)).unwrap();
        std::fs::write(to.join("scripts/b.sql"), "theirs").unwrap();
        assert_eq!(copy_scripts(&from, &to).unwrap(), 1);
        assert_eq!(
            std::fs::read_to_string(to.join("scripts/b.sql")).unwrap(),
            "theirs"
        );
        assert_eq!(
            std::fs::read_to_string(to.join("scripts/reports/a.sql")).unwrap(),
            "select 1"
        );
        assert!(!to.join("scripts/notes.txt").exists());
        assert!(to.join("scripts").join(LINKS_FILE).exists());
    }
}
