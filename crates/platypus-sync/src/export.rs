//! Connections exported to a file (`.platypusconn`) to take to another computer or give to a
//! colleague, and read back through the import dialog.
//!
//! The file is JSON. Each connection goes out as a [`ShareConnection`], the allow-list a
//! shared project uses, so paths, shell commands and secret options stay on this machine.
//! With a password the JSON is sealed in the sync container ([`crate::crypto`]) and may also
//! carry the saved passwords; without one it never does.
//!
//! On reading, the file is untrusted like a shared project's: each connection gets a new id,
//! its `${env:…}` references are held, it cannot make the ODBC driver manager load a library,
//! and agents are kept out until the user lets them in.

use std::collections::{BTreeMap, HashSet};

use platypus_core::Secret;
use platypus_store::{
    AgentAccess, ConnectionFolder, ConnectionStore, ImportedProject, ImportedSecrets,
    SavedConnection, SavedSsh,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::portable::{ShareConnection, hold_shared_references, without_native_loads};
use crate::{Result, SyncError, crypto};

/// The file's extension, without the dot.
pub const EXTENSION: &str = "platypusconn";
/// Names the format inside the JSON.
const FORMAT: &str = "platypusdb-connections";
/// The newest layout this build reads and writes.
pub const FORMAT_VERSION: u32 = 1;

/// What to export.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExportRequest {
    pub connections: Vec<Uuid>,
    /// Keeps the projects and folders the connections are in.
    #[serde(default)]
    pub projects: bool,
    /// Seals the file under this password; `None` writes it in clear.
    #[serde(default)]
    pub password: Option<String>,
    /// Puts the saved passwords in the file; only with `password`.
    #[serde(default)]
    pub credentials: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExportFile {
    format: String,
    version: u32,
    projects: Vec<ExportProject>,
}

/// A project of the file, or the connections outside any (no name).
#[derive(Debug, Default, Serialize, Deserialize)]
struct ExportProject {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// Each folder as its names from the root; parents before their children.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    folders: Vec<Vec<String>>,
    connections: Vec<ShareConnection>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    placement: BTreeMap<Uuid, Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    secrets: BTreeMap<Uuid, ExportSecrets>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ExportSecrets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    password: Option<String>,
    /// The SSH tunnel's password, or the passphrase of its key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ssh: Option<String>,
    /// The same for each jump host behind it, the outermost last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    jumps: Vec<Option<String>>,
}

/// The names from the root of `folder` in `folders`; a broken parent chain stops there.
fn folder_path(folders: &[ConnectionFolder], folder: Uuid) -> Vec<String> {
    let mut path = Vec::new();
    let mut next = Some(folder);
    while let Some(f) = next.and_then(|id| folders.iter().find(|f| f.id == id)) {
        if path.len() > folders.len() {
            break; // a cycle
        }
        path.push(f.name.clone());
        next = f.parent;
    }
    path.reverse();
    path
}

fn secrets_of(store: &ConnectionStore, c: &SavedConnection) -> Result<ExportSecrets> {
    let read = |id| {
        store
            .secret(id)
            .map(|s| s.map(|s| s.expose().to_owned()))
            .map_err(|e| SyncError::Invalid(e.to_string()))
    };
    let mut hops = c
        .ssh
        .iter()
        .flat_map(SavedSsh::chain)
        .map(|s| read(s.secret_id))
        .collect::<Result<Vec<_>>>()?
        .into_iter();
    let ssh = hops.next().flatten();
    let mut jumps: Vec<_> = hops.collect();
    if jumps.iter().all(Option::is_none) {
        jumps.clear();
    }
    Ok(ExportSecrets {
        password: if c.save_password { read(c.id)? } else { None },
        ssh,
        jumps,
    })
}

fn add(
    store: &ConnectionStore,
    project: &mut ExportProject,
    c: &SavedConnection,
    credentials: bool,
) -> Result<()> {
    let mut shared = ShareConnection::from_saved(&store.effective(c), true);
    // The profile's settings are in the connection already; profiles stay here.
    shared.network_profile = None;
    if credentials {
        let secrets = secrets_of(store, c)?;
        if secrets.password.is_some() || secrets.ssh.is_some() || !secrets.jumps.is_empty() {
            project.secrets.insert(c.id, secrets);
        }
    }
    project.connections.push(shared);
    Ok(())
}

fn build(store: &ConnectionStore, request: &ExportRequest) -> Result<ExportFile> {
    let credentials = request.credentials && request.password.is_some();
    let wanted: HashSet<Uuid> = request.connections.iter().copied().collect();
    let mut done = HashSet::new();
    let mut projects = Vec::new();
    if request.projects {
        for p in store.projects() {
            let mut out = ExportProject {
                name: Some(p.name.clone()),
                ..ExportProject::default()
            };
            // A connection in two projects goes with the first.
            for c in p
                .connections
                .iter()
                .filter(|id| wanted.contains(id) && done.insert(**id))
                .filter_map(|id| store.get(*id))
            {
                if let Some(folder) = p.placement.get(&c.id) {
                    let path = folder_path(&p.folders, *folder);
                    for end in 1..=path.len() {
                        if !out.folders.iter().any(|f| f[..] == path[..end]) {
                            out.folders.push(path[..end].to_vec());
                        }
                    }
                    out.placement.insert(c.id, path);
                }
                add(store, &mut out, c, credentials)?;
            }
            if !out.connections.is_empty() {
                projects.push(out);
            }
        }
    }
    let mut rest = ExportProject::default();
    for c in store
        .list()
        .iter()
        .filter(|c| wanted.contains(&c.id) && !done.contains(&c.id))
    {
        add(store, &mut rest, c, credentials)?;
    }
    if !rest.connections.is_empty() {
        projects.push(rest);
    }
    Ok(ExportFile {
        format: FORMAT.to_owned(),
        version: FORMAT_VERSION,
        projects,
    })
}

/// The bytes of the export file `request` asks for. A password shorter than
/// [`crypto::MIN_PASSPHRASE_LEN`] is refused.
pub fn export(store: &ConnectionStore, request: &ExportRequest) -> Result<Vec<u8>> {
    if let Some(password) = &request.password {
        crypto::check_new_passphrase(password)?;
    }
    let json = serde_json::to_vec_pretty(&build(store, request)?)
        .map_err(|e| SyncError::Invalid(e.to_string()))?;
    match &request.password {
        Some(password) => crypto::seal(password, &json),
        None => Ok(json),
    }
}

/// Whether `bytes` is an encrypted export, which [`read`] needs a password for.
#[must_use]
pub fn is_encrypted(bytes: &[u8]) -> bool {
    let text = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    text.trim_ascii_start().first() != Some(&b'{')
}

/// Reads an export file for the import dialog. An encrypted one without `password` gives
/// [`SyncError::PasswordNeeded`]; a wrong password or a changed byte gives
/// [`SyncError::WrongPassphrase`] and no data.
pub fn read(bytes: &[u8], password: Option<&str>) -> Result<Vec<ImportedProject>> {
    let plain;
    let (json, sealed) = if is_encrypted(bytes) {
        let password = password
            .filter(|p| !p.is_empty())
            .ok_or(SyncError::PasswordNeeded)?;
        plain = crypto::open(password, bytes).map_err(|e| match e {
            SyncError::NotSyncFile => not_export(),
            e => e,
        })?;
        (&plain[..], true)
    } else {
        (bytes, false)
    };
    let text = String::from_utf8_lossy(json);
    let file: ExportFile =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| not_export())?;
    if file.format != FORMAT {
        return Err(not_export());
    }
    if file.version > FORMAT_VERSION {
        return Err(SyncError::Invalid(format!(
            "the file was written by a newer PlatypusDB (layout {}): update the app",
            file.version
        )));
    }
    Ok(file
        .projects
        .into_iter()
        .map(|p| imported(p, sealed))
        .collect())
}

fn not_export() -> SyncError {
    SyncError::Invalid("this is not a PlatypusDB connections file".to_owned())
}

/// A project of the file as the import dialog takes it: new ids, and the file's settings
/// checked like a shared project's. Passwords only come from a sealed file.
fn imported(p: ExportProject, sealed: bool) -> ImportedProject {
    let mut out = ImportedProject {
        name: p.name,
        folders: p.folders,
        ..ImportedProject::default()
    };
    let mut secrets = p.secrets;
    for incoming in p.connections {
        let old = incoming.id;
        let mut c = incoming.into_saved(None);
        c.id = Uuid::new_v4();
        c.network_profile = None;
        without_native_loads(None, &mut c);
        hold_shared_references(None, &mut c);
        c.agent_access = AgentAccess::Off;
        if let Some(path) = p.placement.get(&old) {
            out.placement.insert(c.id, path.clone());
        }
        if let Some(s) = secrets.remove(&old).filter(|_| sealed) {
            let secret = |v: Option<String>| v.filter(|v| !v.is_empty()).map(Secret::new);
            let s = ImportedSecrets {
                password: secret(s.password),
                ssh: secret(s.ssh).filter(|_| c.ssh.is_some()),
                jumps: s.jumps.into_iter().map(secret).collect(),
            };
            if s.password.is_some() || s.ssh.is_some() || s.jumps.iter().any(Option::is_some) {
                out.with_password.push(c.id);
                out.secrets.insert(c.id, s);
            }
        }
        out.connections.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use platypus_core::DriverKind;
    use platypus_store::{MemorySecrets, PasswordUpdate, Project, SavedSsh, SshAuthKind};

    use super::*;

    fn store(dir: &Path) -> ConnectionStore {
        ConnectionStore::open(
            dir.join("connections.toml"),
            Arc::new(MemorySecrets::default()),
        )
        .unwrap()
    }

    /// Two connections in a project (one in a nested folder, with a password and an SSH
    /// tunnel), and one outside any project.
    #[allow(clippy::many_single_char_names)]
    fn sample(s: &mut ConnectionStore) -> (Uuid, Uuid, Uuid) {
        let mut a = SavedConnection::new("Prod", DriverKind::Postgres);
        a.host = "db.example.com".to_owned();
        a.user = "alice".to_owned();
        a.save_password = true;
        a.password_command = Some("echo CANARY_CMD".to_owned());
        a.options
            .insert("password".to_owned(), "CANARY_OPT".to_owned());
        a.ssh = Some(SavedSsh {
            host: "bastion".to_owned(),
            port: 22,
            user: "carol".to_owned(),
            auth: SshAuthKind::Password,
            key_path: None,
            secret_id: Uuid::new_v4(),
            jump: Some(Box::new(SavedSsh {
                host: "jump".to_owned(),
                port: 22,
                user: "carol".to_owned(),
                auth: SshAuthKind::Password,
                key_path: None,
                secret_id: Uuid::new_v4(),
                jump: None,
            })),
        });
        let ssh_secret = a.ssh.as_ref().unwrap().secret_id;
        let jump_secret = a.ssh.as_ref().unwrap().jump.as_ref().unwrap().secret_id;
        let b = SavedConnection::new("Local", DriverKind::Sqlite);
        let c = SavedConnection::new("Loose", DriverKind::Sqlite);
        let (aid, bid, cid) = (a.id, b.id, c.id);
        s.save(a, PasswordUpdate::Set(Secret::new("s3cret-pw")))
            .unwrap();
        s.set_ssh_secret(ssh_secret, Some(&Secret::new("ssh-pw")))
            .unwrap();
        s.set_ssh_secret(jump_secret, Some(&Secret::new("jump-pw")))
            .unwrap();
        s.save(b, PasswordUpdate::Keep).unwrap();
        s.save(c, PasswordUpdate::Keep).unwrap();
        let mut p = Project::new("Team", "#112233");
        p.connections = vec![aid, bid];
        let pid = p.id;
        s.save_project(p).unwrap();
        let outer = s.create_folder(pid, "Servers", None).unwrap();
        let inner = s.create_folder(pid, "Prod", Some(outer)).unwrap();
        s.place_connection(pid, aid, Some(inner)).unwrap();
        (aid, bid, cid)
    }

    fn request(ids: &[Uuid], projects: bool, password: Option<&str>) -> ExportRequest {
        ExportRequest {
            connections: ids.to_vec(),
            projects,
            password: password.map(str::to_owned),
            credentials: true,
        }
    }

    #[test]
    fn a_clear_file_has_no_credential_and_keeps_the_projects() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let (a, b, c) = sample(&mut s);
        let bytes = export(&s, &request(&[a, b, c], true, None)).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        for secret in [
            "CANARY",
            "s3cret-pw",
            "ssh-pw",
            "jump-pw",
            "password_command",
            "secret_id",
        ] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
        let read = read(&bytes, None).unwrap();
        assert_eq!(read.len(), 2);
        let team = &read[0];
        assert_eq!(team.name.as_deref(), Some("Team"));
        assert_eq!(
            team.folders,
            [
                vec!["Servers".to_owned()],
                vec!["Servers".to_owned(), "Prod".to_owned()]
            ]
        );
        let prod = &team.connections[0];
        assert_ne!(prod.id, a, "new ids");
        assert_eq!(prod.user, "alice");
        assert_eq!(prod.agent_access, AgentAccess::Off);
        assert_eq!(team.placement[&prod.id], ["Servers", "Prod"]);
        assert!(team.secrets.is_empty() && team.with_password.is_empty());
        assert_eq!(read[1].name, None);
        assert_eq!(read[1].connections[0].name, "Loose");
    }

    #[test]
    fn a_sealed_file_carries_the_passwords_and_needs_its_password() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let (a, _, _) = sample(&mut s);
        let bytes = export(&s, &request(&[a], false, Some("correct horse"))).unwrap();
        assert!(is_encrypted(&bytes));
        assert!(!bytes.windows(9).any(|w| w == b"s3cret-pw"));
        assert!(matches!(read(&bytes, None), Err(SyncError::PasswordNeeded)));
        assert!(matches!(
            read(&bytes, Some("wrong password")),
            Err(SyncError::WrongPassphrase)
        ));
        let mut tampered = bytes.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(read(&tampered, Some("correct horse")).is_err());
        let read = read(&bytes, Some("correct horse")).unwrap();
        let project = &read[0];
        assert_eq!(project.name, None, "no projects asked");
        let id = project.connections[0].id;
        assert_eq!(project.with_password, [id]);
        let secrets = &project.secrets[&id];
        assert_eq!(secrets.password.as_ref().unwrap().expose(), "s3cret-pw");
        assert_eq!(secrets.ssh.as_ref().unwrap().expose(), "ssh-pw");
        assert_eq!(secrets.jumps, [Some(Secret::new("jump-pw"))]);
    }

    #[test]
    fn passwords_need_a_password_and_a_long_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let (a, _, _) = sample(&mut s);
        let clear = export(&s, &request(&[a], false, None)).unwrap();
        assert!(!String::from_utf8(clear).unwrap().contains("s3cret-pw"));
        assert!(matches!(
            export(&s, &request(&[a], false, Some("short"))),
            Err(SyncError::ShortPassphrase)
        ));
    }

    /// A version 1 file as written by hand: what older apps wrote keeps opening.
    #[test]
    fn reads_a_version_1_file() {
        let file = r#"{
          "format": "platypusdb-connections",
          "version": 1,
          "projects": [{
            "name": "Team",
            "folders": [["Servers"]],
            "connections": [{
              "id": "6f1c1d2e-0000-4000-8000-000000000001",
              "name": "Prod",
              "driver": "postgres",
              "host": "db.example.com",
              "port": 5432,
              "database": "sales",
              "user": "alice",
              "agent_access": "read_write",
              "secrets_are_ignored_in_clear": true
            }],
            "placement": { "6f1c1d2e-0000-4000-8000-000000000001": ["Servers"] },
            "secrets": { "6f1c1d2e-0000-4000-8000-000000000001": { "password": "x" } }
          }]
        }"#;
        let read = read(file.as_bytes(), None).unwrap();
        let team = &read[0];
        let c = &team.connections[0];
        assert_eq!(
            (c.host.as_str(), c.port, c.user.as_str()),
            ("db.example.com", Some(5432), "alice")
        );
        assert_eq!(c.agent_access, AgentAccess::Off);
        assert_eq!(team.placement[&c.id], ["Servers"]);
        assert!(team.secrets.is_empty(), "a clear file brings no password");
    }

    #[test]
    fn other_files_are_refused() {
        assert!(
            read(
                b"{\"format\":\"other\",\"version\":1,\"projects\":[]}",
                None
            )
            .is_err()
        );
        assert!(read(b"{}", None).is_err());
        let newer = format!("{{\"format\":\"{FORMAT}\",\"version\":99,\"projects\":[]}}");
        assert!(read(newer.as_bytes(), None).is_err());
        assert!(matches!(
            read(b"PK\x03\x04zip", Some("x")),
            Err(SyncError::Invalid(_))
        ));
    }
}
