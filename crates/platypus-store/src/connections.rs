use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use platypus_core::{ConnectionConfig, DriverKind, Secret, SshAuth, SshConfig, SslMode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Result, SecretStore, StoreError};

/// 2 added projects: an older app would drop them on its first save, so it refuses the file.
const FILE_VERSION: u32 = 2;

/// A connection as the user saved it. Never contains the password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedConnection {
    pub id: Uuid,
    pub name: String,
    pub driver: DriverKind,
    #[serde(default)]
    pub host: String,
    pub port: Option<u16>,
    /// Database name, or the file path for SQLite.
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub ssl_mode: SslMode,
    pub ssl_root_cert: Option<PathBuf>,
    /// Whether the password is kept in the OS keychain.
    #[serde(default)]
    pub save_password: bool,
    /// Blocks statements that modify data (enforced by the app, not the database).
    #[serde(default)]
    pub read_only: bool,
    /// Accent colour for tabs and the navigator, e.g. `#d64545` for production.
    pub color: Option<String>,
    /// Development, test or production: production asks before changes and starts in manual
    /// commit.
    pub connection_type: Option<ConnectionType>,
    /// The SSH server the connection goes through. Before `virtual_keys`: TOML writes plain
    /// values first, then tables.
    pub ssh: Option<SavedSsh>,
    /// Logical keys the user set for tables without a primary key, so the data editor can
    /// identify their rows. Last: TOML writes arrays of tables after plain values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub virtual_keys: Vec<VirtualKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionType {
    Dev,
    Test,
    Prod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SshAuthKind {
    #[default]
    Password,
    Key,
    Agent,
}

const fn ssh_port() -> u16 {
    22
}

/// An SSH server as saved: its password or key passphrase is in the keychain under
/// `secret_id`, never in the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSsh {
    pub host: String,
    #[serde(default = "ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub auth: SshAuthKind,
    /// The private key file, for key logins.
    pub key_path: Option<PathBuf>,
    pub secret_id: Uuid,
    /// The server to reach this one through (`ssh -J`).
    pub jump: Option<Box<SavedSsh>>,
}

impl SavedSsh {
    /// This server and its jump hosts, outermost last.
    pub fn chain(&self) -> impl Iterator<Item = &SavedSsh> {
        std::iter::successors(Some(self), |s| s.jump.as_deref())
    }

    /// The tunnel's settings, with the secrets `secret` finds by id.
    pub fn config(&self, secret: &impl Fn(Uuid) -> Option<Secret>) -> SshConfig {
        let auth = match self.auth {
            SshAuthKind::Password => {
                SshAuth::Password(secret(self.secret_id).unwrap_or_else(|| Secret::new("")))
            }
            SshAuthKind::Key => SshAuth::Key {
                path: self.key_path.clone().unwrap_or_default(),
                passphrase: secret(self.secret_id),
            },
            SshAuthKind::Agent => SshAuth::Agent,
        };
        SshConfig {
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            auth,
            jump: self.jump.as_ref().map(|j| Box::new(j.config(secret))),
        }
    }
}

/// A set of connections worked on together, shown as a square in the project rail. A
/// connection can be in several projects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
    /// One or two characters on the square; computed from the name when empty.
    #[serde(default)]
    pub initials: String,
    /// In the order they were added.
    #[serde(default)]
    pub connections: Vec<Uuid>,
}

impl Project {
    /// Adds the connection (at the end) or takes it out; false when it was already so.
    fn set_member(&mut self, connection: Uuid, member: bool) -> bool {
        let present = self.connections.contains(&connection);
        if member && !present {
            self.connections.push(connection);
        } else if !member && present {
            self.connections.retain(|c| *c != connection);
        } else {
            return false;
        }
        true
    }
}

/// The initials of the first two words ("Cliente Rossi" → "CR"), or the first two letters
/// of a single word ("Interno" → "IN"), upper case.
#[must_use]
pub fn initials_of(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let chars: Vec<char> = match words.as_slice() {
        [] => Vec::new(),
        [word] => word.chars().take(2).collect(),
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
    };
    chars.into_iter().flat_map(char::to_uppercase).collect()
}

/// Trims in place, without a new allocation.
fn trim(s: &mut String) {
    s.truncate(s.trim_end().len());
    let start = s.len() - s.trim_start().len();
    s.drain(..start);
}

/// Drops the ids not in `known`, and repeated ones.
fn keep_known(ids: &mut Vec<Uuid>, known: &HashSet<Uuid>) {
    let mut seen = HashSet::with_capacity(ids.len());
    ids.retain(|id| known.contains(id) && seen.insert(*id));
}

fn is_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Columns that identify a table's rows, set by the user instead of a primary key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualKey {
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
}

impl SavedConnection {
    #[must_use]
    pub fn new(name: impl Into<String>, driver: DriverKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            driver,
            host: "localhost".to_owned(),
            port: driver.default_port(),
            database: String::new(),
            user: String::new(),
            ssl_mode: SslMode::default(),
            ssl_root_cert: None,
            save_password: true,
            read_only: false,
            color: None,
            connection_type: None,
            ssh: None,
            virtual_keys: Vec::new(),
        }
    }

    #[must_use]
    pub fn config(&self, password: Option<Secret>) -> ConnectionConfig {
        let mut config = ConnectionConfig::new(self.driver);
        config.host.clone_from(&self.host);
        config.port = self.port.or(self.driver.default_port());
        config.database.clone_from(&self.database);
        config.user.clone_from(&self.user);
        config.password = password;
        config.ssl_mode = self.ssl_mode;
        config.ssl_root_cert.clone_from(&self.ssl_root_cert);
        config
    }
}

/// How `save` treats the stored password.
#[derive(Debug, Clone)]
pub enum PasswordUpdate {
    Keep,
    Set(Secret),
    Clear,
}

#[derive(Debug, Deserialize)]
struct ConnectionsFile {
    version: u32,
    #[serde(default)]
    projects: Vec<Project>,
    #[serde(default)]
    connections: Vec<SavedConnection>,
}

/// What `write` saves, borrowed from the store.
#[derive(Serialize)]
struct FileOut<'a> {
    version: u32,
    projects: &'a [Project],
    connections: &'a [SavedConnection],
}

#[derive(Debug)]
pub struct ConnectionStore {
    path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    projects: Vec<Project>,
    connections: Vec<SavedConnection>,
}

impl ConnectionStore {
    /// Loads `path`, or starts empty if it does not exist yet.
    pub fn open(path: impl Into<PathBuf>, secrets: Arc<dyn SecretStore>) -> Result<Self> {
        let path = path.into();
        let (mut projects, connections) = match std::fs::read_to_string(&path) {
            Ok(text) => {
                let file: ConnectionsFile =
                    toml::from_str(&text).map_err(|e| StoreError::Format {
                        path: path.clone(),
                        message: e.to_string(),
                    })?;
                if file.version > FILE_VERSION {
                    return Err(StoreError::Format {
                        path,
                        message: format!(
                            "written by a newer PlatypusDB (version {})",
                            file.version
                        ),
                    });
                }
                (file.projects, file.connections)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), Vec::new()),
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        // Files edited by hand: ids of missing connections and empty initials.
        let known = connections.iter().map(|c| c.id).collect();
        for p in &mut projects {
            keep_known(&mut p.connections, &known);
            if p.initials.trim().is_empty() {
                p.initials = initials_of(&p.name);
            }
        }
        Ok(Self {
            path,
            secrets,
            projects,
            connections,
        })
    }

    #[must_use]
    pub fn projects(&self) -> &[Project] {
        &self.projects
    }

    /// Inserts or replaces (by id) a project, after checking it: a name used by no other
    /// project (ignoring case), a `#rrggbb` color, one or two characters of initials (empty
    /// ones are computed from the name). Unknown connection ids are dropped.
    pub fn save_project(&mut self, mut project: Project) -> Result<()> {
        let invalid = |m: String| Err(StoreError::Invalid(m));
        trim(&mut project.name);
        trim(&mut project.initials);
        if project.name.is_empty() {
            return invalid("project name is required".to_owned());
        }
        let lower = project.name.to_lowercase();
        if self
            .projects
            .iter()
            .any(|p| p.id != project.id && p.name.to_lowercase() == lower)
        {
            return invalid(format!(
                "a project named \"{}\" exists already",
                project.name
            ));
        }
        if !is_color(&project.color) {
            return invalid(format!("invalid project color `{}`", project.color));
        }
        if project.initials.is_empty() {
            project.initials = initials_of(&project.name);
        }
        if project.initials.chars().count() > 2 {
            return invalid("initials are one or two characters".to_owned());
        }
        let known = self.connections.iter().map(|c| c.id).collect();
        keep_known(&mut project.connections, &known);
        match self.projects.iter_mut().find(|p| p.id == project.id) {
            Some(existing) => *existing = project,
            None => self.projects.push(project),
        }
        self.write()
    }

    /// Removes a project; its connections stay.
    pub fn delete_project(&mut self, id: Uuid) -> Result<()> {
        let before = self.projects.len();
        self.projects.retain(|p| p.id != id);
        if self.projects.len() == before {
            return Err(StoreError::NotFound(id));
        }
        self.write()
    }

    /// Adds a connection to a project, or (`member` false) takes it out.
    pub fn set_membership(&mut self, connection: Uuid, project: Uuid, member: bool) -> Result<()> {
        if self.get(connection).is_none() {
            return Err(StoreError::NotFound(connection));
        }
        let p = self
            .projects
            .iter_mut()
            .find(|p| p.id == project)
            .ok_or(StoreError::NotFound(project))?;
        if p.set_member(connection, member) {
            self.write()
        } else {
            Ok(())
        }
    }

    /// Makes `projects` the projects a connection is in, and no others.
    pub fn set_projects_of(&mut self, connection: Uuid, projects: &[Uuid]) -> Result<()> {
        if self.get(connection).is_none() {
            return Err(StoreError::NotFound(connection));
        }
        let mut changed = false;
        for p in &mut self.projects {
            changed |= p.set_member(connection, projects.contains(&p.id));
        }
        if changed { self.write() } else { Ok(()) }
    }

    #[must_use]
    pub fn list(&self) -> &[SavedConnection] {
        &self.connections
    }

    #[must_use]
    pub fn get(&self, id: Uuid) -> Option<&SavedConnection> {
        self.connections.iter().find(|c| c.id == id)
    }

    /// Inserts or replaces (by id) a connection and updates its keychain entry.
    pub fn save(&mut self, connection: SavedConnection, password: PasswordUpdate) -> Result<()> {
        if connection.name.trim().is_empty() {
            return Err(StoreError::Invalid(
                "connection name is required".to_owned(),
            ));
        }
        let id = connection.id;
        let save_password = connection.save_password;
        match self.connections.iter_mut().find(|c| c.id == id) {
            Some(existing) => *existing = connection,
            None => self.connections.push(connection),
        }
        self.write()?;
        match password {
            _ if !save_password => self.secrets.delete(id),
            PasswordUpdate::Set(secret) => self.secrets.set(id, &secret),
            PasswordUpdate::Clear => self.secrets.delete(id),
            PasswordUpdate::Keep => Ok(()),
        }
    }

    /// Sets the virtual key of `schema.table`, or removes it when `columns` is empty.
    pub fn set_virtual_key(
        &mut self,
        id: Uuid,
        schema: &str,
        table: &str,
        columns: Vec<String>,
    ) -> Result<()> {
        let connection = self
            .connections
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or(StoreError::NotFound(id))?;
        let keys = &mut connection.virtual_keys;
        keys.retain(|k| k.schema != schema || k.table != table);
        if !columns.is_empty() {
            keys.push(VirtualKey {
                schema: schema.to_owned(),
                table: table.to_owned(),
                columns,
            });
        }
        self.write()
    }

    pub fn delete(&mut self, id: Uuid) -> Result<()> {
        let Some(index) = self.connections.iter().position(|c| c.id == id) else {
            return Err(StoreError::NotFound(id));
        };
        let removed = self.connections.remove(index);
        for p in &mut self.projects {
            p.connections.retain(|c| *c != id);
        }
        self.write()?;
        for ssh in removed.ssh.iter().flat_map(SavedSsh::chain) {
            self.secrets.delete(ssh.secret_id)?;
        }
        self.secrets.delete(id)
    }

    /// Sets (`Some`) or removes (`None`) an SSH password or passphrase by its `secret_id`.
    pub fn set_ssh_secret(&self, secret_id: Uuid, secret: Option<&Secret>) -> Result<()> {
        match secret {
            Some(secret) => self.secrets.set(secret_id, secret),
            None => self.secrets.delete(secret_id),
        }
    }

    /// A saved SSH password or passphrase.
    pub fn ssh_secret(&self, secret_id: Uuid) -> Result<Option<Secret>> {
        self.secrets.get(secret_id)
    }

    /// The saved password, if the connection keeps one.
    pub fn password(&self, id: Uuid) -> Result<Option<Secret>> {
        match self.get(id) {
            Some(c) if c.save_password => self.secrets.get(id),
            Some(_) => Ok(None),
            None => Err(StoreError::NotFound(id)),
        }
    }

    /// Writes atomically: a crash mid-write never leaves a truncated file behind.
    fn write(&self) -> Result<()> {
        let io_error = |source| StoreError::Io {
            path: self.path.clone(),
            source,
        };
        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(io_error)?;
        let text = toml::to_string_pretty(&FileOut {
            version: FILE_VERSION,
            projects: &self.projects,
            connections: &self.connections,
        })
        .map_err(|e| StoreError::Format {
            path: self.path.clone(),
            message: e.to_string(),
        })?;
        let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(io_error)?;
        tmp.write_all(text.as_bytes()).map_err(io_error)?;
        tmp.persist(&self.path).map_err(|e| io_error(e.error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemorySecrets;

    fn store(dir: &Path) -> (ConnectionStore, Arc<MemorySecrets>) {
        let secrets = Arc::new(MemorySecrets::default());
        let store = ConnectionStore::open(dir.join("connections.toml"), secrets.clone()).unwrap();
        (store, secrets)
    }

    fn sample() -> SavedConnection {
        let mut c = SavedConnection::new("Local PG", DriverKind::Postgres);
        c.port = Some(55432);
        c.database = "platypus".to_owned();
        c.user = "platypus".to_owned();
        c.ssl_mode = SslMode::Disable;
        c.color = Some("#d64545".to_owned());
        c
    }

    const NONE: [Uuid; 0] = [];

    fn project(name: &str, connections: Vec<Uuid>) -> Project {
        Project {
            id: Uuid::new_v4(),
            name: name.to_owned(),
            color: "#d08f20".to_owned(),
            initials: String::new(),
            connections,
        }
    }

    #[test]
    fn initials_from_the_first_two_words() {
        assert_eq!(initials_of("Cliente Rossi"), "CR");
        assert_eq!(initials_of("Interno"), "IN");
        assert_eq!(initials_of("  a  b  c "), "AB");
        assert_eq!(initials_of("éclair dé"), "ÉD");
        assert_eq!(initials_of("x"), "X");
    }

    #[test]
    fn projects_round_trip_many_to_many() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, secrets) = store(dir.path());
        let shared = sample();
        s.save(shared.clone(), PasswordUpdate::Keep).unwrap();
        s.save_project(project("Cliente Rossi", vec![shared.id]))
            .unwrap();
        s.save_project(project("Interno", vec![shared.id])).unwrap();

        let text = std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        assert!(text.contains("version = 2"), "{text}");
        assert!(text.contains("[[projects]]"), "{text}");
        let reopened = ConnectionStore::open(dir.path().join("connections.toml"), secrets).unwrap();
        let projects = reopened.projects();
        assert_eq!(projects.len(), 2);
        assert!(projects.iter().all(|p| p.connections == [shared.id]));
        assert_eq!(projects[0].initials, "CR");
        assert_eq!(projects[1].initials, "IN");
    }

    #[test]
    fn reads_a_v1_file_and_writes_v2() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.toml");
        let id = Uuid::new_v4();
        std::fs::write(
            &path,
            format!(
                "version = 1\n\n[[connections]]\nid = \"{id}\"\nname = \"Old\"\ndriver = \"sqlite\"\ndatabase = 'x.db'\nfolder = \"x\"\n"
            ),
        )
        .unwrap();
        let mut s = ConnectionStore::open(&path, Arc::new(MemorySecrets::default())).unwrap();
        assert_eq!(s.projects(), []);
        let old = s.get(id).unwrap().clone();
        s.save(old, PasswordUpdate::Keep).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("version = 2"), "{text}");
        assert!(!text.contains("folder"), "{text}");
    }

    #[test]
    fn deleting_a_connection_removes_it_from_projects() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        let (a, b) = (sample(), sample());
        s.save(a.clone(), PasswordUpdate::Keep).unwrap();
        s.save(b.clone(), PasswordUpdate::Keep).unwrap();
        s.save_project(project("One", vec![a.id, b.id])).unwrap();
        s.save_project(project("Two", vec![a.id])).unwrap();
        s.delete(a.id).unwrap();
        assert_eq!(s.projects()[0].connections, [b.id]);
        assert_eq!(s.projects()[1].connections, NONE);
    }

    #[test]
    fn drops_unknown_connection_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.toml");
        let (known, unknown) = (Uuid::new_v4(), Uuid::new_v4());
        std::fs::write(
            &path,
            format!(
                "version = 2\n\n[[projects]]\nid = \"{}\"\nname = \"P\"\ncolor = \"#123456\"\nconnections = [\"{unknown}\", \"{known}\", \"{known}\"]\n\n[[connections]]\nid = \"{known}\"\nname = \"K\"\ndriver = \"sqlite\"\n",
                Uuid::new_v4()
            ),
        )
        .unwrap();
        let s = ConnectionStore::open(&path, Arc::new(MemorySecrets::default())).unwrap();
        assert_eq!(s.projects()[0].connections, [known]);
        assert_eq!(s.projects()[0].initials, "P");
    }

    #[test]
    fn validates_projects() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        s.save_project(project("Interno", vec![])).unwrap();
        let invalid = |r: Result<()>| matches!(r, Err(StoreError::Invalid(_)));
        assert!(invalid(s.save_project(project("  ", vec![]))));
        assert!(invalid(s.save_project(project("INTERNO", vec![]))));
        let mut red = project("Red", vec![]);
        red.color = "red".to_owned();
        assert!(invalid(s.save_project(red)));
        let mut long = project("Long", vec![]);
        long.initials = "ABC".to_owned();
        assert!(invalid(s.save_project(long)));
        // Renaming a project to its own name in other case is not a clash.
        let mut same = s.projects()[0].clone();
        same.name = "interno".to_owned();
        s.save_project(same).unwrap();
    }

    #[test]
    fn set_membership_adds_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        let c = sample();
        s.save(c.clone(), PasswordUpdate::Keep).unwrap();
        s.save_project(project("P", vec![])).unwrap();
        let p = s.projects()[0].id;
        s.set_membership(c.id, p, true).unwrap();
        s.set_membership(c.id, p, true).unwrap();
        assert_eq!(s.projects()[0].connections, [c.id]);
        s.set_membership(c.id, p, false).unwrap();
        assert_eq!(s.projects()[0].connections, NONE);
        assert!(matches!(
            s.set_membership(Uuid::new_v4(), p, true),
            Err(StoreError::NotFound(_))
        ));
    }

    #[test]
    fn set_projects_of_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        let c = sample();
        s.save(c.clone(), PasswordUpdate::Keep).unwrap();
        s.save_project(project("A", vec![c.id])).unwrap();
        s.save_project(project("B", vec![])).unwrap();
        let (a, b) = (s.projects()[0].id, s.projects()[1].id);
        s.set_projects_of(c.id, &[b]).unwrap();
        assert_eq!(s.projects()[0].connections, NONE);
        assert_eq!(s.projects()[1].connections, [c.id]);
        s.set_projects_of(c.id, &[a, b]).unwrap();
        assert!(s.projects().iter().all(|p| p.connections == [c.id]));
    }

    #[test]
    fn round_trips_without_leaking_the_password() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, secrets) = store(dir.path());
        let conn = sample();
        s.save(conn.clone(), PasswordUpdate::Set(Secret::new("hunter2")))
            .unwrap();

        let text = std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        assert!(!text.contains("hunter2"));
        assert!(text.contains("ssl_mode = \"disable\""), "{text}");

        let reopened = ConnectionStore::open(dir.path().join("connections.toml"), secrets).unwrap();
        assert_eq!(reopened.list(), std::slice::from_ref(&conn));
        assert_eq!(
            reopened.password(conn.id).unwrap(),
            Some(Secret::new("hunter2"))
        );

        let config = reopened
            .get(conn.id)
            .unwrap()
            .config(Some(Secret::new("x")));
        assert_eq!(config.port, Some(55432));
        assert_eq!(config.ssl_mode, SslMode::Disable);
    }

    #[test]
    fn updates_in_place_and_honours_password_updates() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        let mut conn = sample();
        s.save(conn.clone(), PasswordUpdate::Set(Secret::new("a")))
            .unwrap();

        conn.name = "Renamed".to_owned();
        s.save(conn.clone(), PasswordUpdate::Keep).unwrap();
        assert_eq!(s.list().len(), 1);
        assert_eq!(s.list()[0].name, "Renamed");
        assert_eq!(s.password(conn.id).unwrap(), Some(Secret::new("a")));

        conn.save_password = false;
        s.save(conn.clone(), PasswordUpdate::Set(Secret::new("ignored")))
            .unwrap();
        assert_eq!(s.password(conn.id).unwrap(), None);
    }

    #[test]
    fn sets_and_removes_virtual_keys() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, secrets) = store(dir.path());
        let conn = sample();
        s.save(conn.clone(), PasswordUpdate::Keep).unwrap();
        s.set_virtual_key(conn.id, "public", "log", vec!["at".into(), "seq".into()])
            .unwrap();
        s.set_virtual_key(conn.id, "public", "log", vec!["id".into()])
            .unwrap();
        s.set_virtual_key(conn.id, "public", "other", vec!["x".into()])
            .unwrap();
        s.set_virtual_key(conn.id, "public", "other", Vec::new())
            .unwrap();

        let reopened = ConnectionStore::open(dir.path().join("connections.toml"), secrets).unwrap();
        assert_eq!(
            reopened.list()[0].virtual_keys,
            [VirtualKey {
                schema: "public".into(),
                table: "log".into(),
                columns: vec!["id".into()],
            }]
        );
        assert!(matches!(
            s.set_virtual_key(Uuid::new_v4(), "a", "b", Vec::new()),
            Err(StoreError::NotFound(_))
        ));
    }

    #[test]
    fn keeps_ssh_settings_and_their_secrets_apart() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, secrets) = store(dir.path());
        let mut conn = sample();
        conn.connection_type = Some(ConnectionType::Prod);
        let jump = SavedSsh {
            host: "bastion".into(),
            port: 2222,
            user: "ops".into(),
            auth: SshAuthKind::Agent,
            key_path: None,
            secret_id: Uuid::new_v4(),
            jump: None,
        };
        let ssh = SavedSsh {
            host: "db-box".into(),
            port: 22,
            user: "me".into(),
            auth: SshAuthKind::Password,
            key_path: None,
            secret_id: Uuid::new_v4(),
            jump: Some(Box::new(jump)),
        };
        conn.ssh = Some(ssh.clone());
        s.save(conn.clone(), PasswordUpdate::Keep).unwrap();
        s.set_virtual_key(conn.id, "public", "log", vec!["id".into()])
            .unwrap();
        s.set_ssh_secret(ssh.secret_id, Some(&Secret::new("ssh-pw")))
            .unwrap();

        let text = std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        assert!(!text.contains("ssh-pw"), "{text}");
        let reopened =
            ConnectionStore::open(dir.path().join("connections.toml"), secrets.clone()).unwrap();
        assert_eq!(reopened.list()[0].ssh, conn.ssh);
        assert_eq!(
            reopened.list()[0].connection_type,
            Some(ConnectionType::Prod)
        );

        let config = ssh.config(&|id| reopened.ssh_secret(id).unwrap());
        assert_eq!(config.auth, SshAuth::Password(Secret::new("ssh-pw")));
        assert_eq!(config.jump.unwrap().auth, SshAuth::Agent);

        s.delete(conn.id).unwrap();
        assert_eq!(secrets.get(ssh.secret_id).unwrap(), None);
    }

    #[test]
    fn delete_removes_secret() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, secrets) = store(dir.path());
        let conn = sample();
        s.save(conn.clone(), PasswordUpdate::Set(Secret::new("a")))
            .unwrap();
        s.delete(conn.id).unwrap();
        assert_eq!(s.list(), []);
        assert_eq!(secrets.get(conn.id).unwrap(), None);
        assert!(matches!(s.delete(conn.id), Err(StoreError::NotFound(_))));
    }

    #[test]
    fn rejects_files_from_newer_versions_and_empty_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.toml");
        std::fs::write(&path, "version = 99\n").unwrap();
        let err = ConnectionStore::open(&path, Arc::new(MemorySecrets::default())).unwrap_err();
        assert!(matches!(err, StoreError::Format { .. }));
        std::fs::write(&path, "version = 3\n").unwrap();
        let err = ConnectionStore::open(&path, Arc::new(MemorySecrets::default())).unwrap_err();
        assert!(err.to_string().contains("newer PlatypusDB"), "{err}");

        let (mut s, _) = store(&dir.path().join("other"));
        let mut conn = sample();
        conn.name = "  ".to_owned();
        assert!(matches!(
            s.save(conn, PasswordUpdate::Keep),
            Err(StoreError::Invalid(_))
        ));
    }
}
