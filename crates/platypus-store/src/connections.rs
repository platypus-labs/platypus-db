use std::collections::{BTreeMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use platypus_core::{
    AUTH, Auth, ConnectionConfig, DriverKind, Secret, SshAuth, SshConfig, SslMode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Result, SecretStore, StoreError};

mod folders;
mod imports;

pub use imports::{ImportInto, ImportRequest, ImportTarget};

/// 2 added projects, 3 network profiles, 4 the agents' access, 5 folders, 6 drivers loaded
/// at run time and their `options`: an older app would drop them on its first save, so it
/// refuses the file. 7: the order of a project's connections and folders is the user's;
/// older files are put in the order they were shown (see `ordered_as_shown`).
const FILE_VERSION: u32 = 7;

/// The first version whose projects keep their connections and folders in the user's order.
const ARRANGED_VERSION: u32 = 7;

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
    /// The certificate and key presented to servers that authenticate clients by certificate.
    pub ssl_client_cert: Option<PathBuf>,
    pub ssl_client_key: Option<PathBuf>,
    /// Whether the password is kept in the OS keychain.
    #[serde(default)]
    pub save_password: bool,
    /// A shell command that prints the password, run at each connection (e.g. a password
    /// manager's CLI). Takes precedence over `.pgpass` and the keychain.
    pub password_command: Option<String>,
    /// Looks the password up in `.pgpass` (`PGPASSFILE`), as libpq does.
    #[serde(default)]
    pub pgpass: bool,
    /// A network profile whose SSH tunnel and SSL settings replace the connection's own.
    pub network_profile: Option<Uuid>,
    /// Blocks statements that modify data (enforced by the app, not the database).
    #[serde(default)]
    pub read_only: bool,
    /// Accent colour for tabs and the navigator, e.g. `#d64545` for production.
    pub color: Option<String>,
    /// Development, test or production: production asks before changes and starts in manual
    /// commit.
    pub connection_type: Option<ConnectionType>,
    /// What external agents (MCP clients, the CLI) may do on the connection; see
    /// [`SavedConnection::effective_agent_access`].
    #[serde(default)]
    pub agent_access: AgentAccess,
    /// Whether the value panel formats a text column holding JSON as it does a JSON column.
    #[serde(default, skip_serializing_if = "TextJson::is_format")]
    pub text_json: TextJson,
    /// How the form enters the address: host, port and database, or one URL. Only the
    /// form's: it opens again as it was left.
    #[serde(default, skip_serializing_if = "ConnectBy::is_host")]
    pub connect_by: ConnectBy,
    /// The SSH server the connection goes through. Before `virtual_keys`: TOML writes plain
    /// values first, then tables.
    pub ssh: Option<SavedSsh>,
    /// Default schema, time zone and bootstrap statements, run after every connect.
    #[serde(default, skip_serializing_if = "SavedInit::is_empty")]
    pub init: SavedInit,
    /// What the app does not do with the connection's data, whatever the page offers.
    #[serde(default, skip_serializing_if = "Restrictions::is_none")]
    pub restrictions: Restrictions,
    /// What a driver needs beyond the common fields, e.g. Snowflake's `warehouse`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
    /// Logical keys the user set for tables without a primary key, so the data editor can
    /// identify their rows. Last: TOML writes arrays of tables after plain values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub virtual_keys: Vec<VirtualKey>,
}

/// Restrictions on a connection, enforced by the app (as `read_only` is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Restrictions {
    /// Its data leaves it neither to files (exports, dumps) nor to other databases, nor to
    /// agents.
    #[serde(default)]
    pub no_export: bool,
    /// Its rows change only by SQL: the data editor, imports and generated rows refuse.
    #[serde(default)]
    pub no_data_editing: bool,
}

impl Restrictions {
    #[must_use]
    pub fn is_none(&self) -> bool {
        *self == Self::default()
    }
}

/// What the value panel does with JSON found in a text column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextJson {
    /// Indented, as in a JSON column.
    #[default]
    Format,
    /// As it is written.
    Plain,
}

impl TextJson {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_format(&self) -> bool {
        *self == Self::Format
    }
}

/// How the connection form enters the address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectBy {
    /// The host, port and database fields.
    #[default]
    Host,
    /// One connection URL.
    Url,
}

impl ConnectBy {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_host(&self) -> bool {
        *self == Self::Host
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionType {
    Dev,
    Test,
    Prod,
}

/// What external agents may do on a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAccess {
    /// Agents do not see the connection.
    Off,
    /// Agents read the schema and run queries that change nothing.
    #[default]
    ReadOnly,
    /// Agents may also change data and schema.
    ReadWrite,
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

/// What runs after connecting, as saved; `statements` is a script the app splits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedInit {
    pub schema: Option<String>,
    pub time_zone: Option<String>,
    #[serde(default)]
    pub statements: String,
}

impl SavedInit {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.schema.is_none() && self.time_zone.is_none() && self.statements.trim().is_empty()
    }
}

/// An SSH tunnel and SSL settings shared by several connections: changing it changes them
/// all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkProfile {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub ssl_mode: SslMode,
    pub ssl_root_cert: Option<PathBuf>,
    pub ssl_client_cert: Option<PathBuf>,
    pub ssl_client_key: Option<PathBuf>,
    pub ssh: Option<SavedSsh>,
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
    /// In the order the navigator shows them, which the user arranges; new ones at the end.
    #[serde(default)]
    pub connections: Vec<Uuid>,
    /// Folders of the project's connections, nested through `parent`; the folders of one
    /// parent are shown in this order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<ConnectionFolder>,
    /// The folder of each connection in one; connections not here are at the root.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub placement: BTreeMap<Uuid, Uuid>,
}

/// A folder of projects in the rail. A project is in one folder at most.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFolder {
    pub id: Uuid,
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
    /// In order.
    #[serde(default)]
    pub projects: Vec<Uuid>,
    #[serde(default)]
    pub collapsed: bool,
}

/// A folder of connections inside a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionFolder {
    pub id: Uuid,
    pub name: String,
    /// `None` at the project's root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Uuid>,
}

impl Project {
    /// An empty project; the initials are computed from the name.
    #[must_use]
    pub fn new(name: &str, color: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_owned(),
            color: color.to_owned(),
            initials: initials_of(name),
            connections: Vec::new(),
            folders: Vec::new(),
            placement: BTreeMap::new(),
        }
    }

    /// Adds the connection (at the end) or takes it out; false when it was already so.
    fn set_member(&mut self, connection: Uuid, member: bool) -> bool {
        let present = self.connections.contains(&connection);
        if member && !present {
            self.connections.push(connection);
        } else if !member && present {
            self.connections.retain(|c| *c != connection);
            self.placement.remove(&connection);
        } else {
            return false;
        }
        true
    }

    /// The order a file older than `ARRANGED_VERSION` was shown in: connections in the order
    /// of the saved list, folders by name.
    fn ordered_as_shown(&mut self, list: &[SavedConnection]) {
        self.connections
            .sort_by_key(|id| list.iter().position(|c| c.id == *id));
        self.folders
            .sort_by_cached_key(|f| (shown_order(&f.name), f.name.clone()));
    }
}

/// A name's place in the old UI's sort (`localeCompare`), near enough: accents and case
/// do not count, and punctuation and spaces come before digits, digits before letters.
fn shown_order(name: &str) -> Vec<(u8, char)> {
    use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
    name.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| {
            let class = if c.is_alphabetic() {
                2
            } else {
                u8::from(c.is_numeric())
            };
            (class, c)
        })
        .collect()
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

/// Why `save` refuses a host such as `root:secret@db:3306`, without repeating it.
const HOST_WITH_LOGIN: &str = "the host is not valid: it holds a login (`user:password@`), \
     a port or other keys; put the user, password and port in their own fields";

/// Drops the ids not in `known`, and repeated ones.
fn keep_known(ids: &mut Vec<Uuid>, known: &HashSet<Uuid>) {
    let mut seen = HashSet::with_capacity(ids.len());
    ids.retain(|id| known.contains(id) && seen.insert(*id));
}

/// Takes the item at `from` out of `list` and puts it back just before the first item
/// `is_before` picks, or at the end when none does.
fn reposition<T>(list: &mut Vec<T>, from: usize, is_before: impl Fn(&T) -> bool) {
    let item = list.remove(from);
    let at = list.iter().position(is_before).unwrap_or(list.len());
    list.insert(at, item);
}

fn check_client_cert(cert: Option<&PathBuf>, key: Option<&PathBuf>) -> Result<()> {
    if cert.is_some() == key.is_some() {
        Ok(())
    } else {
        Err(StoreError::Invalid(
            "a client certificate needs both the certificate and its key".to_owned(),
        ))
    }
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
            ssl_client_cert: None,
            ssl_client_key: None,
            save_password: true,
            password_command: None,
            pgpass: false,
            network_profile: None,
            read_only: false,
            color: None,
            connection_type: None,
            agent_access: AgentAccess::default(),
            text_json: TextJson::Format,
            connect_by: ConnectBy::Host,
            ssh: None,
            init: SavedInit::default(),
            restrictions: Restrictions::default(),
            options: BTreeMap::new(),
            virtual_keys: Vec::new(),
        }
    }

    #[must_use]
    pub fn config(&self, password: Option<Secret>) -> ConnectionConfig {
        let mut config = ConnectionConfig::new(self.driver);
        // Spaces copied with a host name are no part of it.
        self.host.trim().clone_into(&mut config.host);
        config.port = self.port.or(self.driver.default_port());
        config.database.clone_from(&self.database);
        config.user.clone_from(&self.user);
        config.password = password;
        config.ssl_mode = self.ssl_mode;
        config.ssl_root_cert.clone_from(&self.ssl_root_cert);
        config.ssl_client_cert.clone_from(&self.ssl_client_cert);
        config.ssl_client_key.clone_from(&self.ssl_client_key);
        config.init.schema.clone_from(&self.init.schema);
        config.init.time_zone.clone_from(&self.init.time_zone);
        config.options.clone_from(&self.options);
        config
    }

    /// What agents may do in fact: production and read-only connections never let them
    /// write, whatever was chosen, and one whose data is not exported keeps them out.
    #[must_use]
    pub fn effective_agent_access(&self) -> AgentAccess {
        // Rows an agent reads leave the app.
        if self.restrictions.no_export {
            return AgentAccess::Off;
        }
        let capped = self.read_only || self.connection_type == Some(ConnectionType::Prod);
        if capped {
            self.agent_access.min(AgentAccess::ReadOnly)
        } else {
            self.agent_access
        }
    }

    /// The password is asked at each connection: none is saved, looked up or printed by a
    /// command.
    #[must_use]
    pub fn asks_password(&self) -> bool {
        self.driver != DriverKind::Sqlite
            && !self.save_password
            && !self.pgpass
            && self.password_command.is_none()
            && !self.signs_in_without_password()
    }

    /// Whether it signs in with a token from a cloud's CLI or with Windows' own sign-in: no
    /// password is asked, saved or looked up.
    #[must_use]
    pub fn signs_in_without_password(&self) -> bool {
        Auth::from_option(self.options.get(AUTH).map(String::as_str))
            .is_ok_and(|auth| auth != Auth::Password)
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
    project_folders: Vec<ProjectFolder>,
    #[serde(default)]
    network_profiles: Vec<NetworkProfile>,
    #[serde(default)]
    connections: Vec<SavedConnection>,
}

/// What `write` saves, borrowed from the store.
#[derive(Serialize)]
struct FileOut<'a> {
    version: u32,
    projects: &'a [Project],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    project_folders: &'a [ProjectFolder],
    network_profiles: &'a [NetworkProfile],
    connections: &'a [SavedConnection],
}

#[derive(Debug)]
pub struct ConnectionStore {
    path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    projects: Vec<Project>,
    project_folders: Vec<ProjectFolder>,
    network_profiles: Vec<NetworkProfile>,
    connections: Vec<SavedConnection>,
}

impl ConnectionStore {
    /// Loads `path`, or starts empty if it does not exist yet.
    pub fn open(path: impl Into<PathBuf>, secrets: Arc<dyn SecretStore>) -> Result<Self> {
        let path = path.into();
        let (version, mut projects, mut project_folders, network_profiles, connections) =
            match std::fs::read_to_string(&path) {
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
                    (
                        file.version,
                        file.projects,
                        file.project_folders,
                        file.network_profiles,
                        file.connections,
                    )
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    (FILE_VERSION, Vec::new(), Vec::new(), Vec::new(), Vec::new())
                }
                Err(source) => return Err(StoreError::Io { path, source }),
            };
        // Files edited by hand: ids of missing connections and empty initials.
        let known = connections.iter().map(|c| c.id).collect();
        for p in &mut projects {
            keep_known(&mut p.connections, &known);
            if p.initials.trim().is_empty() {
                p.initials = initials_of(&p.name);
            }
            p.drop_dangling();
            if version < ARRANGED_VERSION {
                p.ordered_as_shown(&connections);
            }
        }
        let mut seen = HashSet::new();
        for f in &mut project_folders {
            f.projects
                .retain(|id| projects.iter().any(|p| p.id == *id) && seen.insert(*id));
        }
        project_folders.retain(|f| !f.projects.is_empty());
        Ok(Self {
            path,
            secrets,
            projects,
            project_folders,
            network_profiles,
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
        // Folders change through their own operations: the project dialog does not send them.
        let at = self.projects.iter().position(|p| p.id == project.id);
        if let Some(existing) = at.map(|i| &mut self.projects[i]) {
            project.folders = std::mem::take(&mut existing.folders);
            project.placement = std::mem::take(&mut existing.placement);
        }
        project.drop_dangling();
        match at {
            Some(i) => self.projects[i] = project,
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
        self.leave_project_folder(id);
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

    /// Adds several connections to a project, writing the file once.
    pub fn add_to_project(&mut self, project: Uuid, connections: &[Uuid]) -> Result<()> {
        if let Some(missing) = connections.iter().find(|c| self.get(**c).is_none()) {
            return Err(StoreError::NotFound(*missing));
        }
        let p = self
            .projects
            .iter_mut()
            .find(|p| p.id == project)
            .ok_or(StoreError::NotFound(project))?;
        let mut changed = false;
        for c in connections {
            changed |= p.set_member(*c, true);
        }
        if changed { self.write() } else { Ok(()) }
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

    /// Moves a connection in the saved list, which All connections shows: just before
    /// `before`, or to the end for `None`.
    pub fn reorder_connection(&mut self, connection: Uuid, before: Option<Uuid>) -> Result<()> {
        let from = self
            .connections
            .iter()
            .position(|c| c.id == connection)
            .ok_or(StoreError::NotFound(connection))?;
        if let Some(before) = before {
            if before == connection {
                return Err(StoreError::Invalid(
                    "a connection cannot go before itself".to_owned(),
                ));
            }
            self.get(before).ok_or(StoreError::NotFound(before))?;
        }
        reposition(&mut self.connections, from, |c| Some(c.id) == before);
        self.write()
    }

    #[must_use]
    pub fn get(&self, id: Uuid) -> Option<&SavedConnection> {
        self.connections.iter().find(|c| c.id == id)
    }

    #[must_use]
    pub fn network_profiles(&self) -> &[NetworkProfile] {
        &self.network_profiles
    }

    /// Inserts or replaces (by id) a network profile, after checking it: a name used by no
    /// other profile (ignoring case), and a client certificate with its key.
    pub fn save_network_profile(&mut self, mut profile: NetworkProfile) -> Result<()> {
        trim(&mut profile.name);
        if profile.name.is_empty() {
            return Err(StoreError::Invalid("profile name is required".to_owned()));
        }
        let lower = profile.name.to_lowercase();
        if self
            .network_profiles
            .iter()
            .any(|p| p.id != profile.id && p.name.to_lowercase() == lower)
        {
            return Err(StoreError::Invalid(format!(
                "a network profile named \"{}\" exists already",
                profile.name
            )));
        }
        check_client_cert(
            profile.ssl_client_cert.as_ref(),
            profile.ssl_client_key.as_ref(),
        )?;
        match self
            .network_profiles
            .iter_mut()
            .find(|p| p.id == profile.id)
        {
            Some(existing) => *existing = profile,
            None => self.network_profiles.push(profile),
        }
        self.write()
    }

    /// Deletes a profile no connection uses, with its SSH secrets.
    pub fn delete_network_profile(&mut self, id: Uuid) -> Result<()> {
        let users: Vec<&str> = self
            .connections
            .iter()
            .filter(|c| c.network_profile == Some(id))
            .map(|c| c.name.as_str())
            .collect();
        if !users.is_empty() {
            return Err(StoreError::Invalid(format!(
                "the profile is used by {}",
                users.join(", ")
            )));
        }
        let Some(index) = self.network_profiles.iter().position(|p| p.id == id) else {
            return Err(StoreError::NotFound(id));
        };
        let removed = self.network_profiles.remove(index);
        self.write()?;
        for ssh in removed.ssh.iter().flat_map(SavedSsh::chain) {
            self.secrets.delete(ssh.secret_id)?;
        }
        Ok(())
    }

    /// The connection with its network profile's SSH tunnel and SSL settings in place of
    /// its own.
    #[must_use]
    pub fn effective(&self, connection: &SavedConnection) -> SavedConnection {
        let mut out = connection.clone();
        let profile = connection
            .network_profile
            .and_then(|id| self.network_profiles.iter().find(|p| p.id == id));
        if let Some(p) = profile {
            out.ssl_mode = p.ssl_mode;
            out.ssl_root_cert.clone_from(&p.ssl_root_cert);
            out.ssl_client_cert.clone_from(&p.ssl_client_cert);
            out.ssl_client_key.clone_from(&p.ssl_client_key);
            out.ssh.clone_from(&p.ssh);
        }
        out
    }

    /// The URL of a saved connection, with its network profile's SSL settings and without
    /// its password (see [`ConnectionConfig::to_url`]).
    pub fn url_of(&self, id: Uuid) -> Result<String> {
        let saved = self.get(id).ok_or(StoreError::NotFound(id))?;
        self.effective(saved).config(None).to_url().ok_or_else(|| {
            StoreError::Invalid(format!("{} connections have no URL", saved.driver.name()))
        })
    }

    /// Inserts or replaces (by id) a connection and updates its keychain entry.
    pub fn save(
        &mut self,
        mut connection: SavedConnection,
        password: PasswordUpdate,
    ) -> Result<()> {
        trim(&mut connection.host);
        self.check_connection(&connection)?;
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

    /// What `save` refuses: no name, a host that holds a login or other keys (its password
    /// would be saved in clear), half a client certificate, an unknown network profile.
    fn check_connection(&self, connection: &SavedConnection) -> Result<()> {
        if connection.name.trim().is_empty() {
            return Err(StoreError::Invalid(
                "connection name is required".to_owned(),
            ));
        }
        // A variable of the host (`${env:DB_HOST}`) stands for a plain name here, and is
        // checked again once it is read, when the connection opens.
        if !connection.driver.is_embedded() {
            let mut config = connection.config(None);
            config.host = platypus_core::without_references(&config.host);
            config
                .check_host()
                .map_err(|_| StoreError::Invalid(HOST_WITH_LOGIN.to_owned()))?;
        }
        check_client_cert(
            connection.ssl_client_cert.as_ref(),
            connection.ssl_client_key.as_ref(),
        )?;
        if let Some(id) = connection.network_profile
            && !self.network_profiles.iter().any(|p| p.id == id)
        {
            return Err(StoreError::Invalid("unknown network profile".to_owned()));
        }
        Ok(())
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
            p.set_member(id, false);
        }
        self.write()?;
        for ssh in removed.ssh.iter().flat_map(SavedSsh::chain) {
            self.secrets.delete(ssh.secret_id)?;
        }
        self.secrets.delete(id)
    }

    /// The ids under which secrets may be saved: connections that keep their password, and
    /// the SSH servers of connections and network profiles.
    #[must_use]
    pub fn secret_ids(&self) -> Vec<Uuid> {
        let ssh = self
            .connections
            .iter()
            .filter_map(|c| c.ssh.as_ref())
            .chain(self.network_profiles.iter().filter_map(|p| p.ssh.as_ref()))
            .flat_map(SavedSsh::chain)
            .map(|s| s.secret_id);
        self.connections
            .iter()
            .filter(|c| c.save_password)
            .map(|c| c.id)
            .chain(ssh)
            .collect()
    }

    /// Moves every saved secret to `to` and uses it from now on; the old store keeps none.
    pub fn move_secrets(&mut self, to: Arc<dyn SecretStore>) -> Result<()> {
        let ids = self.secret_ids();
        for &id in &ids {
            if let Some(secret) = self.secrets.get(id)? {
                to.set(id, &secret)?;
            }
        }
        for &id in &ids {
            self.secrets.delete(id)?;
        }
        self.secrets = to;
        Ok(())
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

    /// Saves a copy of a connection, named "… (copy)", next to it in the list and in the
    /// same projects and folders, with copies of its saved password and SSH secrets.
    pub fn duplicate(&mut self, id: Uuid) -> Result<SavedConnection> {
        let source = self.get(id).ok_or(StoreError::NotFound(id))?;
        let mut copy = source.clone();
        copy.id = Uuid::new_v4();
        copy.name = self.copy_name(&source.name);
        let mut secrets = Vec::new();
        if copy.save_password
            && let Some(secret) = self.secrets.get(id)?
        {
            secrets.push((copy.id, secret));
        }
        let mut hop = copy.ssh.as_mut();
        while let Some(ssh) = hop {
            let old = ssh.secret_id;
            ssh.secret_id = Uuid::new_v4();
            if let Some(secret) = self.secrets.get(old)? {
                secrets.push((ssh.secret_id, secret));
            }
            hop = ssh.jump.as_deref_mut();
        }
        let at = self
            .connections
            .iter()
            .position(|c| c.id == id)
            .map_or(self.connections.len(), |i| i + 1);
        self.connections.insert(at, copy.clone());
        for p in &mut self.projects {
            if let Some(i) = p.connections.iter().position(|c| *c == id) {
                p.connections.insert(i + 1, copy.id);
                if let Some(folder) = p.placement.get(&id).copied() {
                    p.placement.insert(copy.id, folder);
                }
            }
        }
        self.write()?;
        for (secret_id, secret) in secrets {
            self.secrets.set(secret_id, &secret)?;
        }
        Ok(copy)
    }

    /// `name (copy)`, or `name (copy 2)`… when that one is taken.
    fn copy_name(&self, name: &str) -> String {
        let taken = |n: &str| self.connections.iter().any(|c| c.name == n);
        let mut candidate = format!("{name} (copy)");
        let mut n = 2;
        while taken(&candidate) {
            candidate = format!("{name} (copy {n})");
            n += 1;
        }
        candidate
    }

    /// Saves the password of a connection that keeps it in the keychain, as typed at its
    /// prompt when none was saved; does nothing for the others.
    pub fn set_password(&self, id: Uuid, password: &Secret) -> Result<()> {
        match self.get(id) {
            Some(c) if c.save_password => self.secrets.set(id, password),
            Some(_) => Ok(()),
            None => Err(StoreError::NotFound(id)),
        }
    }

    /// Whether a password is saved for the connection; locked saved passwords count as
    /// saved.
    pub fn has_password(&self, id: Uuid) -> Result<bool> {
        match self.password(id) {
            Ok(secret) => Ok(secret.is_some()),
            Err(StoreError::Locked) => Ok(true),
            Err(e) => Err(e),
        }
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
            project_folders: &self.project_folders,
            network_profiles: &self.network_profiles,
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

    #[test]
    fn older_folders_keep_the_order_they_were_shown_in() {
        let mut names = ["Zahn", "ärzte", "_tmp", "Bravo", "2024", "Arzt"];
        names.sort_by_cached_key(|n| (shown_order(n), (*n).to_owned()));
        assert_eq!(names, ["_tmp", "2024", "Arzt", "ärzte", "Bravo", "Zahn"]);
    }

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
            folders: Vec::new(),
            placement: BTreeMap::new(),
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
        assert!(text.contains("version = 7"), "{text}");
        assert!(text.contains("[[projects]]"), "{text}");
        let reopened = ConnectionStore::open(dir.path().join("connections.toml"), secrets).unwrap();
        let projects = reopened.projects();
        assert_eq!(projects.len(), 2);
        assert!(projects.iter().all(|p| p.connections == [shared.id]));
        assert_eq!(projects[0].initials, "CR");
        assert_eq!(projects[1].initials, "IN");
    }

    #[test]
    fn json_in_text_columns_is_formatted_unless_turned_off() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        let mut c = sample();
        s.save(c.clone(), PasswordUpdate::Keep).unwrap();
        let file = || std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        assert!(!file().contains("text_json"), "the default is not written");
        assert_eq!(s.get(c.id).unwrap().text_json, TextJson::Format);
        c.text_json = TextJson::Plain;
        s.save(c.clone(), PasswordUpdate::Keep).unwrap();
        assert!(file().contains("text_json = \"plain\""), "{}", file());
        let (again, _) = store(dir.path());
        assert_eq!(again.get(c.id).unwrap().text_json, TextJson::Plain);
    }

    #[test]
    fn a_host_from_the_environment_is_saved_as_written() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        let mut from_env = sample();
        from_env.host = "${env:DB_HOST}".to_owned();
        s.save(from_env.clone(), PasswordUpdate::Keep).unwrap();
        assert_eq!(s.get(from_env.id).unwrap().host, "${env:DB_HOST}");
        let mut login_from_env = sample();
        login_from_env.host = "root:${env:PW}@db".to_owned();
        assert!(s.save(login_from_env, PasswordUpdate::Keep).is_err());
        let mut login = sample();
        login.host = "root:secret@db".to_owned();
        assert!(s.save(login, PasswordUpdate::Keep).is_err());
    }

    #[test]
    fn url_of_a_saved_connection_has_no_password() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        let c = sample();
        s.save(c.clone(), PasswordUpdate::Set(Secret::new("hunter2")))
            .unwrap();
        let url = s.url_of(c.id).unwrap();
        assert_eq!(
            url,
            "postgres://platypus@localhost:55432/platypus?sslmode=disable"
        );
        assert_eq!(ConnectionConfig::from_url(&url).unwrap(), c.config(None));
        let snow = SavedConnection::new("Snow", DriverKind::Snowflake);
        s.save(snow.clone(), PasswordUpdate::Keep).unwrap();
        assert!(matches!(s.url_of(snow.id), Err(StoreError::Invalid(_))));
        assert!(matches!(
            s.url_of(Uuid::new_v4()),
            Err(StoreError::NotFound(_))
        ));
    }

    #[test]
    fn options_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        let mut c = SavedConnection::new("Snow", DriverKind::Snowflake);
        c.options
            .insert("warehouse".to_owned(), "COMPUTE_WH".to_owned());
        let id = c.id;
        s.save(c, PasswordUpdate::Keep).unwrap();
        let (again, _) = store(dir.path());
        let saved = again.get(id).unwrap();
        assert_eq!(
            saved.options.get("warehouse").map(String::as_str),
            Some("COMPUTE_WH")
        );
        assert_eq!(
            saved
                .config(None)
                .options
                .get("warehouse")
                .map(String::as_str),
            Some("COMPUTE_WH")
        );
    }

    #[test]
    fn reads_a_v1_file_and_writes_v7() {
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
        assert!(text.contains("version = 7"), "{text}");
        assert!(!text.contains("folder"), "{text}");
        // Older files leave agents to read only.
        assert!(text.contains("agent_access = \"read_only\""), "{text}");
    }

    #[test]
    fn keeps_the_form_in_url_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.toml");
        let mut s = ConnectionStore::open(&path, Arc::new(MemorySecrets::default())).unwrap();
        let host = SavedConnection::new("Host", DriverKind::Postgres);
        let mut url = SavedConnection::new("Url", DriverKind::MySql);
        url.connect_by = ConnectBy::Url;
        s.save(host.clone(), PasswordUpdate::Keep).unwrap();
        s.save(url.clone(), PasswordUpdate::Keep).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        // Host mode, the default, is not written: older files load as it.
        assert_eq!(text.matches("connect_by").count(), 1, "{text}");
        assert!(text.contains("connect_by = \"url\""), "{text}");
        let s = ConnectionStore::open(&path, Arc::new(MemorySecrets::default())).unwrap();
        assert_eq!(s.get(host.id).unwrap().connect_by, ConnectBy::Host);
        assert_eq!(s.get(url.id).unwrap().connect_by, ConnectBy::Url);
    }

    fn profile(name: &str) -> NetworkProfile {
        NetworkProfile {
            id: Uuid::new_v4(),
            name: name.to_owned(),
            ssl_mode: SslMode::VerifyFull,
            ssl_root_cert: Some("ca.pem".into()),
            ssl_client_cert: Some("client.crt".into()),
            ssl_client_key: Some("client.key".into()),
            ssh: None,
        }
    }

    #[test]
    fn network_profiles_replace_the_connections_settings() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        let shared = profile("Office VPN");
        s.save_network_profile(shared.clone()).unwrap();
        assert!(s.save_network_profile(profile(" office vpn ")).is_err());
        let mut half = profile("Half");
        half.ssl_client_key = None;
        assert!(s.save_network_profile(half).is_err());

        let mut conn = sample();
        conn.network_profile = Some(shared.id);
        conn.init.schema = Some("sales".to_owned());
        conn.init.statements = "SET a = 1;
SET b = 2;"
            .to_owned();
        conn.password_command = Some("pass show db".to_owned());
        s.save(conn.clone(), PasswordUpdate::Keep).unwrap();
        let effective = s.effective(&conn);
        assert_eq!(effective.ssl_mode, SslMode::VerifyFull);
        assert_eq!(effective.ssl_client_cert, shared.ssl_client_cert);
        let config = effective.config(None);
        assert_eq!(config.init.schema.as_deref(), Some("sales"));
        assert_eq!(config.ssl_root_cert, shared.ssl_root_cert);

        // In use: not deleted.
        let err = s.delete_network_profile(shared.id).unwrap_err();
        assert!(err.to_string().contains("Local PG"), "{err}");
        let reopened = ConnectionStore::open(
            dir.path().join("connections.toml"),
            Arc::new(MemorySecrets::default()),
        )
        .unwrap();
        assert_eq!(reopened.network_profiles(), std::slice::from_ref(&shared));
        assert_eq!(reopened.get(conn.id), Some(&conn));

        conn.network_profile = None;
        s.save(conn.clone(), PasswordUpdate::Keep).unwrap();
        s.delete_network_profile(shared.id).unwrap();
        assert_eq!(s.network_profiles(), []);
        conn.network_profile = Some(shared.id);
        assert!(s.save(conn, PasswordUpdate::Keep).is_err());
    }

    #[test]
    fn moves_every_saved_secret() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, old) = store(dir.path());
        let conn = sample();
        s.save(conn.clone(), PasswordUpdate::Set(Secret::new("pw")))
            .unwrap();
        let mut shared = profile("Shared");
        let ssh = SavedSsh {
            host: "bastion".to_owned(),
            port: 22,
            user: "ops".to_owned(),
            auth: SshAuthKind::Password,
            key_path: None,
            secret_id: Uuid::new_v4(),
            jump: None,
        };
        shared.ssh = Some(ssh.clone());
        s.save_network_profile(shared).unwrap();
        s.set_ssh_secret(ssh.secret_id, Some(&Secret::new("ssh")))
            .unwrap();

        let new = Arc::new(MemorySecrets::default());
        s.move_secrets(new.clone()).unwrap();
        assert_eq!(old.get(conn.id).unwrap(), None);
        assert_eq!(old.get(ssh.secret_id).unwrap(), None);
        assert_eq!(new.get(conn.id).unwrap().unwrap().expose(), "pw");
        assert_eq!(
            s.ssh_secret(ssh.secret_id).unwrap().unwrap().expose(),
            "ssh"
        );
    }

    #[test]
    fn asks_the_password_only_without_a_source() {
        let mut c = sample();
        c.save_password = false;
        assert!(c.asks_password());
        c.pgpass = true;
        assert!(!c.asks_password());
        c.pgpass = false;
        c.password_command = Some("x".to_owned());
        assert!(!c.asks_password());
        c.password_command = None;
        c.options.insert(AUTH.to_owned(), "aws_iam".to_owned());
        assert!(!c.asks_password());
        assert!(c.signs_in_without_password());
        c.options.insert(AUTH.to_owned(), "password".to_owned());
        assert!(c.asks_password());
    }

    #[test]
    fn agents_never_write_to_production_or_read_only_connections() {
        let mut c = sample();
        assert_eq!(c.agent_access, AgentAccess::ReadOnly, "the default");
        c.agent_access = AgentAccess::ReadWrite;
        assert_eq!(c.effective_agent_access(), AgentAccess::ReadWrite);
        c.connection_type = Some(ConnectionType::Prod);
        assert_eq!(c.effective_agent_access(), AgentAccess::ReadOnly);
        c.agent_access = AgentAccess::Off;
        assert_eq!(c.effective_agent_access(), AgentAccess::Off);
        c.connection_type = Some(ConnectionType::Test);
        c.agent_access = AgentAccess::ReadWrite;
        c.read_only = true;
        assert_eq!(c.effective_agent_access(), AgentAccess::ReadOnly);
    }

    #[test]
    fn restrictions_round_trip_and_keep_agents_out() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, secrets) = store(dir.path());
        let mut conn = sample();
        s.save(conn.clone(), PasswordUpdate::Keep).unwrap();
        let text = std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        assert!(
            !text.contains("restrictions"),
            "none is not written: {text}"
        );
        conn.restrictions = Restrictions {
            no_export: true,
            no_data_editing: true,
        };
        conn.ssh = Some(SavedSsh {
            host: "bastion".into(),
            port: 22,
            user: "ops".into(),
            auth: SshAuthKind::Agent,
            key_path: None,
            secret_id: Uuid::new_v4(),
            jump: None,
        });
        s.save(conn.clone(), PasswordUpdate::Keep).unwrap();
        let reopened = ConnectionStore::open(dir.path().join("connections.toml"), secrets).unwrap();
        assert_eq!(reopened.get(conn.id), Some(&conn));
        conn.agent_access = AgentAccess::ReadWrite;
        assert_eq!(conn.effective_agent_access(), AgentAccess::Off);
    }

    #[test]
    fn agent_access_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, secrets) = store(dir.path());
        let mut conn = sample();
        conn.agent_access = AgentAccess::Off;
        conn.ssh = Some(SavedSsh {
            host: "bastion".into(),
            port: 22,
            user: "ops".into(),
            auth: SshAuthKind::Agent,
            key_path: None,
            secret_id: Uuid::new_v4(),
            jump: None,
        });
        s.save(conn.clone(), PasswordUpdate::Keep).unwrap();
        let text = std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        assert!(text.contains("agent_access = \"off\""), "{text}");
        let reopened = ConnectionStore::open(dir.path().join("connections.toml"), secrets).unwrap();
        assert_eq!(reopened.get(conn.id), Some(&conn));
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
    fn reorder_connection_moves_in_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        let ids: Vec<Uuid> = ["a", "b", "c"]
            .into_iter()
            .map(|name| {
                let c = SavedConnection::new(name, DriverKind::Sqlite);
                let id = c.id;
                s.save(c, PasswordUpdate::Keep).unwrap();
                id
            })
            .collect();
        let order = |s: &ConnectionStore| s.list().iter().map(|c| c.id).collect::<Vec<_>>();
        s.reorder_connection(ids[2], Some(ids[0])).unwrap();
        assert_eq!(order(&s), [ids[2], ids[0], ids[1]]);
        s.reorder_connection(ids[2], None).unwrap();
        assert_eq!(order(&s), ids);
        assert!(matches!(
            s.reorder_connection(ids[0], Some(ids[0])),
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            s.reorder_connection(ids[0], Some(Uuid::new_v4())),
            Err(StoreError::NotFound(_))
        ));
        let (again, _) = store(dir.path());
        assert_eq!(order(&again), ids);
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
    fn add_to_project_adds_each_once() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        let (a, mut b) = (sample(), sample());
        b.id = Uuid::new_v4();
        s.save(a.clone(), PasswordUpdate::Keep).unwrap();
        s.save(b.clone(), PasswordUpdate::Keep).unwrap();
        s.save_project(project("P", vec![a.id])).unwrap();
        let p = s.projects()[0].id;
        s.add_to_project(p, &[a.id, b.id]).unwrap();
        assert_eq!(s.projects()[0].connections, [a.id, b.id]);
        assert!(matches!(
            s.add_to_project(p, &[Uuid::new_v4()]),
            Err(StoreError::NotFound(_))
        ));
        assert!(matches!(
            s.add_to_project(Uuid::new_v4(), &[a.id]),
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
    fn hosts_are_trimmed_and_logins_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = store(dir.path());
        let mut conn = sample();
        conn.host = " db.example.com ".to_owned();
        assert_eq!(conn.config(None).host, "db.example.com");
        s.save(conn.clone(), PasswordUpdate::Keep).unwrap();
        assert_eq!(s.get(conn.id).unwrap().host, "db.example.com");

        conn.host = "root:s3cret@db.example.com:3306".to_owned();
        let err = s
            .save(conn.clone(), PasswordUpdate::Keep)
            .unwrap_err()
            .to_string();
        assert!(!err.contains("s3cret"), "{err}");
        let text = std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        assert!(!text.contains("s3cret"), "{text}");
        // An embedded database has no host to check.
        let mut lite = SavedConnection::new("lite", DriverKind::Sqlite);
        lite.host = "a@b".to_owned();
        s.save(lite, PasswordUpdate::Keep).unwrap();
    }

    #[test]
    fn duplicates_with_the_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, secrets) = store(dir.path());
        let mut conn = sample();
        conn.ssh = Some(SavedSsh {
            host: "bastion".into(),
            port: 22,
            user: "ops".into(),
            auth: SshAuthKind::Password,
            key_path: None,
            secret_id: Uuid::new_v4(),
            jump: None,
        });
        s.save(conn.clone(), PasswordUpdate::Set(Secret::new("pw")))
            .unwrap();
        let ssh_id = conn.ssh.as_ref().unwrap().secret_id;
        s.set_ssh_secret(ssh_id, Some(&Secret::new("ssh-pw")))
            .unwrap();
        let other = SavedConnection::new("other", DriverKind::Postgres);
        s.save(other.clone(), PasswordUpdate::Keep).unwrap();
        let mut project = Project::new("P", "#000000");
        project.connections = vec![conn.id, other.id];
        s.save_project(project.clone()).unwrap();

        let copy = s.duplicate(conn.id).unwrap();
        assert_ne!(copy.id, conn.id);
        assert_eq!(copy.name, format!("{} (copy)", conn.name));
        assert_eq!(
            (copy.host.as_str(), copy.port),
            (conn.host.as_str(), conn.port)
        );
        let copied_ssh = copy.ssh.as_ref().unwrap().secret_id;
        assert_ne!(copied_ssh, ssh_id);
        assert_eq!(secrets.get(copy.id).unwrap().unwrap().expose(), "pw");
        assert_eq!(secrets.get(copied_ssh).unwrap().unwrap().expose(), "ssh-pw");
        let ids: Vec<Uuid> = s.list().iter().map(|c| c.id).collect();
        assert_eq!(ids, [conn.id, copy.id, other.id]);
        assert_eq!(s.projects()[0].connections, [conn.id, copy.id, other.id]);
        // Deleting the copy leaves the original's secrets.
        s.delete(copy.id).unwrap();
        assert_eq!(secrets.get(conn.id).unwrap().unwrap().expose(), "pw");
        assert_eq!(secrets.get(ssh_id).unwrap().unwrap().expose(), "ssh-pw");
        assert_eq!(
            s.duplicate(conn.id).unwrap().name,
            format!("{} (copy)", conn.name)
        );
        assert_eq!(
            s.duplicate(conn.id).unwrap().name,
            format!("{} (copy 2)", conn.name)
        );
        assert!(s.has_password(conn.id).unwrap());
        assert!(!s.has_password(other.id).unwrap());
        s.set_password(other.id, &Secret::new("typed")).unwrap();
        assert!(s.has_password(other.id).unwrap());
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
        std::fs::write(&path, format!("version = {}\n", FILE_VERSION + 1)).unwrap();
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
