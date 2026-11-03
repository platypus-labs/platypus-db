//! Connections imported from other tools: DBeaver (`data-sources.json`, or the projects of
//! a `.dbp` export), DataGrip (`dataSources.xml`), pgAdmin (an exported `servers.json`) and
//! libpq's `pg_service.conf`. Folders come along. DBeaver's saved user names and passwords
//! come along too (its `credentials-config.json`); the passwords are kept apart from what
//! the dialog sees, in [`ImportedProject::secrets`]. Other connections ask for their
//! password on first use.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use platypus_core::{AUTH, DriverKind, JdbcUrl, Secret, SslMode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::connections::{ConnectionType, SavedConnection, SavedSsh, SshAuthKind};
use crate::{Result, StoreError};
use credentials::{Credentials, Saved};

mod credentials;
mod dbp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportSource {
    Dbeaver,
    Datagrip,
    Pgadmin,
    PgService,
    /// A PlatypusDB export (`.platypusconn`), read by `platypus-sync`, which can decrypt it.
    Platypus,
}

/// A file found where a tool keeps its connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportFile {
    pub source: ImportSource,
    pub path: PathBuf,
}

/// What a file holds for one project: its connections and their folders. Files of a single
/// set of connections give one project without a name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedProject {
    pub name: Option<String>,
    /// Each folder as its names from the root; parents before their children.
    pub folders: Vec<Vec<String>>,
    pub connections: Vec<SavedConnection>,
    /// The folder of each connection not at the root.
    pub placement: BTreeMap<Uuid, Vec<String>>,
    /// The connections of the file that are not imported.
    #[serde(default)]
    pub skipped: Vec<SkippedConnection>,
    /// What to know before using a connection, by connection id.
    #[serde(default)]
    pub notes: BTreeMap<Uuid, Vec<ImportNote>>,
    /// What to know about the project as a whole.
    #[serde(default)]
    pub project_notes: Vec<ProjectNote>,
    /// The connections that come with a saved password.
    #[serde(default)]
    pub with_password: Vec<Uuid>,
    /// The saved passwords, by connection id. Never serialized: they do not leave the
    /// backend, which keeps them until the import is saved.
    #[serde(skip)]
    pub secrets: BTreeMap<Uuid, ImportedSecrets>,
}

/// The passwords saved for an imported connection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportedSecrets {
    pub password: Option<Secret>,
    /// The SSH tunnel's password, or the passphrase of its key.
    pub ssh: Option<Secret>,
    /// The same for the jump hosts behind it, in the order of [`crate::SavedSsh::chain`].
    pub jumps: Vec<Option<Secret>>,
}

/// Something to know about a whole project of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectNote {
    /// DBeaver keeps no saved users or passwords for it.
    NoSavedLogins,
    /// Its saved users and passwords could not be read: DBeaver may protect them with a
    /// master password.
    SavedLoginsUnreadable,
}

/// A connection of the file to a database the import does not read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedConnection {
    pub name: String,
    /// The tool's own ids of the database and its driver, e.g. DBeaver's `db2` and
    /// `db2_luw`.
    pub provider: String,
    pub driver: String,
}

/// Something an imported connection needs before it can connect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportNote {
    /// It logged in with Windows (integrated, NTLM or Kerberos), which only the app on
    /// Windows can do.
    WindowsAuth,
    /// It logged in with Microsoft Entra ID (Azure Active Directory).
    EntraAuth,
    /// A SQL Server named instance without a port: the port it listens on is needed.
    NamedInstance { instance: String },
    /// The database file is not there (a relative path stays as it was written): it must be
    /// chosen before connecting.
    MissingFile { path: String },
}

impl ImportedProject {
    fn new(connections: Vec<SavedConnection>) -> Self {
        Self {
            connections,
            ..Self::default()
        }
    }

    /// Adds the folder at `path` (`A/B`), and its parents; returns its names.
    fn add_folder(&mut self, path: &str) -> Vec<String> {
        let parts: Vec<String> = path
            .split('/')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect();
        for end in 1..=parts.len() {
            if !self.folders.iter().any(|f| f[..] == parts[..end]) {
                self.folders.push(parts[..end].to_vec());
            }
        }
        self.folders.sort();
        parts
    }

    /// Adds what another file of the same project holds.
    fn merge(&mut self, other: ImportedProject) {
        for folder in other.folders {
            self.add_folder(&folder.join("/"));
        }
        self.connections.extend(other.connections);
        self.placement.extend(other.placement);
        self.skipped.extend(other.skipped);
        self.notes.extend(other.notes);
        for note in other.project_notes {
            self.note(note);
        }
        self.with_password.extend(other.with_password);
        self.secrets.extend(other.secrets);
    }

    fn note(&mut self, note: ProjectNote) {
        if !self.project_notes.contains(&note) {
            self.project_notes.push(note);
        }
    }

    /// Keeps the passwords saved for a connection.
    fn keep_secrets(&mut self, connection: Uuid, secrets: ImportedSecrets) {
        if secrets.password.is_some() {
            self.with_password.push(connection);
        }
        if secrets != ImportedSecrets::default() {
            self.secrets.insert(connection, secrets);
        }
    }

    /// Puts a connection in the folder at `path`, made if missing.
    fn place(&mut self, connection: Uuid, path: &str) {
        let parts = self.add_folder(path);
        if !parts.is_empty() {
            self.placement.insert(connection, parts);
        }
    }
}

fn home() -> Option<PathBuf> {
    std::env::home_dir()
}

/// The files of the tools installed for this user, where they keep them by default.
/// DataGrip and pgAdmin keep theirs per project or export, so they are picked by hand.
#[must_use]
pub fn detect() -> Vec<ImportFile> {
    let mut candidates = Vec::new();
    let dbeaver = PathBuf::from("DBeaverData/workspace6/General/.dbeaver/data-sources.json");
    if let Ok(appdata) = std::env::var("APPDATA") {
        candidates.push((ImportSource::Dbeaver, Path::new(&appdata).join(&dbeaver)));
        candidates.push((
            ImportSource::PgService,
            Path::new(&appdata).join("postgresql/.pg_service.conf"),
        ));
    }
    if let Some(home) = home() {
        candidates.push((
            ImportSource::Dbeaver,
            home.join(".local/share").join(&dbeaver),
        ));
        candidates.push((ImportSource::Dbeaver, home.join("Library").join(&dbeaver)));
        candidates.push((ImportSource::PgService, home.join(".pg_service.conf")));
    }
    if let Ok(file) = std::env::var("PGSERVICEFILE") {
        candidates.push((ImportSource::PgService, PathBuf::from(file)));
    }
    let mut found: Vec<ImportFile> = candidates
        .into_iter()
        .filter(|(_, path)| path.is_file())
        .map(|(source, path)| ImportFile { source, path })
        .collect();
    found.dedup();
    found
}

/// The projects in `path`, as `source` writes them, with new ids. A DBeaver file is either
/// a `data-sources.json`, with its saved logins in `credentials-config.json` beside it, or
/// a `.dbp` export with several projects.
pub fn import(source: ImportSource, path: &Path) -> Result<Vec<ImportedProject>> {
    let read = |p: &Path| {
        std::fs::read(p).map_err(|source| StoreError::Io {
            path: p.to_owned(),
            source,
        })
    };
    // Windows editors save a byte order mark, which is not part of the text.
    let read_text = |p: &Path| {
        read(p).map(|b| {
            String::from_utf8_lossy(&b)
                .trim_start_matches('\u{feff}')
                .to_owned()
        })
    };
    let invalid = |message: String| StoreError::Format {
        path: path.to_owned(),
        message,
    };
    let one = |connections| Ok(vec![ImportedProject::new(connections)]);
    let mut projects = match source {
        ImportSource::Dbeaver => {
            let bytes = read(path)?;
            if bytes.starts_with(b"PK\x03\x04") {
                dbp::projects(&bytes).map_err(invalid)
            } else {
                dbeaver(&String::from_utf8_lossy(&bytes), &credentials_beside(path))
                    .map(|p| vec![p])
                    .map_err(invalid)
            }
        }
        ImportSource::Pgadmin => pgadmin(&read_text(path)?).map(|p| vec![p]).map_err(invalid),
        ImportSource::PgService => one(pg_service(&read_text(path)?)),
        ImportSource::Datagrip => {
            // User names are in the local file next to the shared one.
            let local = path.with_file_name("dataSources.local.xml");
            let users = if local.is_file() {
                read_text(&local)?
            } else {
                String::new()
            };
            one(datagrip(&read_text(path)?, &users))
        }
        ImportSource::Platypus => Err(invalid(
            "PlatypusDB exports are read by platypus_sync::export::read".to_owned(),
        )),
    }?;
    let base = project_folder(path);
    for project in &mut projects {
        locate_files(project, base);
    }
    Ok(projects)
}

/// Where relative database files are looked for: the folder of a `.dbp` export, the project
/// folder of a `data-sources.json` (`<project>/.dbeaver/`) or a `dataSources.xml`
/// (`<project>/.idea/`).
fn project_folder(path: &Path) -> Option<&Path> {
    let parent = path.parent()?;
    match parent.file_name().and_then(|n| n.to_str()) {
        Some(".dbeaver" | ".idea") => parent.parent(),
        _ => Some(parent),
    }
}

/// Resolves the relative paths of database files against `base` when the file is there, and
/// notes the files that are not: opening them would create an empty database instead.
fn locate_files(project: &mut ImportedProject, base: Option<&Path>) {
    for c in &mut project.connections {
        let path = c.database.as_str();
        let uri = path.starts_with("file:") || path.contains("://");
        if !c.driver.is_embedded() || path.is_empty() || path == ":memory:" || uri {
            continue;
        }
        let file = Path::new(path);
        if file.has_root() || has_drive(path) {
            if file.exists() {
                continue;
            }
        } else if let Some(found) = base.map(|b| b.join(file)).filter(|f| f.is_file()) {
            c.database = found.to_string_lossy().into_owned();
            continue;
        }
        let note = ImportNote::MissingFile {
            path: c.database.clone(),
        };
        project.notes.entry(c.id).or_default().push(note);
    }
}

/// `C:/x.db` or `C:\x.db`: absolute on Windows, written so on any system.
fn has_drive(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\')
}

/// The saved logins of the workspace whose `data-sources.json` is at `path`.
fn credentials_beside(path: &Path) -> Credentials {
    match std::fs::read(path.with_file_name("credentials-config.json")) {
        Ok(bytes) => Credentials::of(Some(&bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Credentials::Missing,
        Err(_) => Credentials::Unreadable,
    }
}

fn connection(name: &str, driver: DriverKind) -> SavedConnection {
    let mut c = SavedConnection::new(name, driver);
    // Ask for the password on first use, unless the import saves one.
    c.save_password = false;
    c
}

/// A JDBC URL of a database the import reads; others are left out like unknown drivers.
fn read_jdbc(url: &str) -> Option<JdbcUrl> {
    JdbcUrl::parse(url).ok().filter(|u| {
        matches!(
            u.config.driver,
            DriverKind::Postgres
                | DriverKind::MySql
                | DriverKind::Sqlite
                | DriverKind::SqlServer
                | DriverKind::ClickHouse
                | DriverKind::DuckDb
                | DriverKind::Oracle
        )
    })
}

/// Takes driver, host, port, database, user and TLS mode from a JDBC URL; not the password.
fn apply_jdbc(c: &mut SavedConnection, url: &JdbcUrl) {
    let config = &url.config;
    c.driver = config.driver;
    config.host.clone_into(&mut c.host);
    c.port = config.port;
    config.database.clone_into(&mut c.database);
    config.user.clone_into(&mut c.user);
    c.ssl_mode = config.ssl_mode;
}

fn text(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A `data-sources.json`, with the project's saved logins.
fn dbeaver(json: &str, credentials: &Credentials) -> Result<ImportedProject, String> {
    // A byte order mark is not JSON.
    let root: Value =
        serde_json::from_str(json.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
    let mut project = ImportedProject::new(Vec::new());
    for folder in root
        .get("folders")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|f| f.keys())
    {
        project.add_folder(folder);
    }
    let Some(connections) = root.get("connections").and_then(Value::as_object) else {
        return Ok(project);
    };
    match credentials {
        Credentials::Missing => project.note(ProjectNote::NoSavedLogins),
        Credentials::Unreadable => project.note(ProjectNote::SavedLoginsUnreadable),
        Credentials::Read(_) => {}
    }
    for (id, entry) in connections {
        match dbeaver_connection(entry, credentials.get(id)) {
            Ok((c, notes, secrets)) => {
                if let Some(folder) = text(entry, "folder") {
                    project.place(c.id, &folder);
                }
                if !notes.is_empty() {
                    project.notes.insert(c.id, notes);
                }
                project.keep_secrets(c.id, secrets);
                project.connections.push(c);
            }
            Err(skipped) => project.skipped.push(skipped),
        }
    }
    project.connections.sort_by(|a, b| a.name.cmp(&b.name));
    project.skipped.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(project)
}

/// One entry of `connections`, with what DBeaver saved for it, or what it is when its
/// database is not one of these.
fn dbeaver_connection(
    entry: &Value,
    saved: Option<&Saved>,
) -> Result<(SavedConnection, Vec<ImportNote>, ImportedSecrets), SkippedConnection> {
    let config = entry.get("configuration").unwrap_or(&Value::Null);
    let name = text(entry, "name").unwrap_or_else(|| "DBeaver connection".to_owned());
    let provider = text(entry, "provider").unwrap_or_default();
    let driver_id = text(entry, "driver").unwrap_or_default();
    // A connection set by URL keeps only the URL, without host, port and database.
    let url = text(config, "url").and_then(|u| read_jdbc(&u));
    let Some(driver) = url
        .as_ref()
        .map(|u| u.config.driver)
        .or_else(|| dbeaver_driver(&provider, &driver_id))
    else {
        return Err(SkippedConnection {
            name,
            provider,
            driver: driver_id,
        });
    };
    let mut c = connection(&name, driver);
    c.port = driver.default_port();
    if let Some(url) = &url {
        apply_jdbc(&mut c, url);
    }
    // DBeaver connects with the URL when the connection is set by URL, else with the
    // fields: their leftovers in the other mode do not count.
    let by_url = url.is_some() && text(config, "configurationType").as_deref() == Some("URL");
    if !by_url {
        dbeaver_address(&mut c, config);
    }
    if let Some(user) = text(config, "user") {
        c.user = user;
    }
    c.read_only = entry
        .get("read-only")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // PostgreSQL's "Show all databases".
    c.show_all_databases = config
        .get("provider-properties")
        .and_then(|p| p.get("@dbeaver-show-non-default-db@"))
        .is_some_and(|v| v == true || v == "true");
    c.connection_type = match text(config, "type").as_deref() {
        Some("dev") => Some(ConnectionType::Dev),
        Some("test") => Some(ConnectionType::Test),
        Some("prod") => Some(ConnectionType::Prod),
        _ => None,
    };
    c.ssh = dbeaver_ssh(config);
    let secrets = saved.map(|s| apply_saved(&mut c, s)).unwrap_or_default();
    let notes = if driver == DriverKind::SqlServer {
        let port_given =
            url.as_ref().is_some_and(|u| u.port_given) || !by_url && text(config, "port").is_some();
        sqlserver_details(&mut c, config, url.as_ref(), port_given)
    } else {
        Vec::new()
    };
    Ok((c, notes, secrets))
}

/// Host, port and database from the fields of DBeaver's connection dialog.
fn dbeaver_address(c: &mut SavedConnection, config: &Value) {
    if let Some(host) = text(config, "host") {
        c.host = host;
    }
    if let Some(port) = text(config, "port").and_then(|p| p.parse().ok()) {
        c.port = Some(port);
    }
    if let Some(database) = text(config, "database") {
        c.database = database;
    }
}

/// Takes the user names DBeaver saved for the connection and its SSH tunnel; returns the
/// passwords, which are saved only if the user wants them.
fn apply_saved(c: &mut SavedConnection, saved: &Saved) -> ImportedSecrets {
    if let Some(user) = &saved.connection.user {
        user.clone_into(&mut c.user);
    }
    let ssh = c.ssh.as_mut().and_then(|ssh| {
        if let Some(user) = &saved.ssh.user {
            user.clone_into(&mut ssh.user);
        }
        // The agent holds its own keys.
        match ssh.auth {
            SshAuthKind::Agent => None,
            SshAuthKind::Password | SshAuthKind::Key => saved.ssh.password.clone(),
        }
    });
    ImportedSecrets {
        password: saved.connection.password.clone(),
        ssh,
        jumps: Vec::new(),
    }
}

/// The database of a DBeaver connection by its provider and driver ids, for one without a
/// JDBC URL this import reads.
fn dbeaver_driver(provider: &str, driver: &str) -> Option<DriverKind> {
    let driver = driver.to_ascii_lowercase();
    match provider.to_ascii_lowercase().as_str() {
        "postgresql" => Some(DriverKind::Postgres),
        "mysql" | "mariadb" => Some(DriverKind::MySql),
        "sqlserver" | "mssql" => Some(DriverKind::SqlServer),
        "clickhouse" => Some(DriverKind::ClickHouse),
        "duckdb" => Some(DriverKind::DuckDb),
        "oracle" => Some(DriverKind::Oracle),
        "libsql" | "turso" => Some(DriverKind::Libsql),
        "snowflake" => Some(DriverKind::Snowflake),
        _ if driver.contains("clickhouse") => Some(DriverKind::ClickHouse),
        _ if driver.contains("duckdb") => Some(DriverKind::DuckDb),
        _ if driver.contains("oracle") => Some(DriverKind::Oracle),
        _ if driver.contains("libsql") => Some(DriverKind::Libsql),
        _ if driver.contains("snowflake") => Some(DriverKind::Snowflake),
        _ if driver.contains("sqlite") => Some(DriverKind::Sqlite),
        _ if sqlserver_driver(&driver) => Some(DriverKind::SqlServer),
        _ => None,
    }
}

/// DBeaver's SQL Server drivers, also under its generic provider: `mssql_jdbc_ms_new`,
/// `microsoft`, `jtds_sqlserver`, `azure_ms_new`; not jTDS for Sybase.
fn sqlserver_driver(driver: &str) -> bool {
    !driver.contains("sybase")
        && (driver.contains("mssql")
            || driver.contains("sqlserver")
            || driver.contains("jtds")
            || driver == "microsoft"
            || driver.starts_with("azure") && driver.contains("_ms"))
}

/// The SSH tunnel of a DBeaver connection, when it is on.
fn dbeaver_ssh(config: &Value) -> Option<SavedSsh> {
    let tunnel = config
        .get("handlers")
        .and_then(|h| h.get("ssh_tunnel"))
        .filter(|t| t.get("enabled").and_then(Value::as_bool).unwrap_or(false))?;
    let props = tunnel.get("properties")?;
    Some(SavedSsh {
        host: text(props, "host")?,
        port: text(props, "port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(22),
        user: text(props, "user").unwrap_or_default(),
        auth: match text(props, "authType").as_deref() {
            Some("PUBLIC_KEY") => SshAuthKind::Key,
            Some("AGENT") => SshAuthKind::Agent,
            _ => SshAuthKind::Password,
        },
        key_path: text(props, "keyPath").map(PathBuf::from),
        secret_id: Uuid::new_v4(),
        jump: None,
    })
}

/// Splits a named instance off the host (`SRV\SQLEXPRESS`), takes the user from the
/// driver properties, and notes what the connection needs here that it did not in DBeaver.
fn sqlserver_details(
    c: &mut SavedConnection,
    config: &Value,
    url: Option<&JdbcUrl>,
    port_given: bool,
) -> Vec<ImportNote> {
    let mut notes = Vec::new();
    let (host, instance) = JdbcUrl::split_instance(&c.host);
    c.host = host;
    let instance = instance.or_else(|| url.and_then(|u| u.instance.clone()));
    // With a port, the instance is not looked up, as for the JDBC driver.
    if let Some(instance) = instance
        && !port_given
    {
        notes.push(ImportNote::NamedInstance { instance });
    }
    let mut props = url.map(|u| u.properties.clone()).unwrap_or_default();
    // The driver properties set in DBeaver's connection dialog.
    if let Some(own) = config.get("properties").and_then(Value::as_object) {
        props.extend(
            own.iter()
                .filter_map(|(k, v)| Some((k.to_ascii_lowercase(), v.as_str()?.to_owned()))),
        );
    }
    if c.user.is_empty()
        && let Some(user) = props.get("user")
    {
        user.clone_into(&mut c.user);
    }
    if let Some(note) = sqlserver_login(&dbeaver_auth(config), &props) {
        // Kept as the connection's way to sign in; the note says what it needs.
        let auth = match note {
            ImportNote::WindowsAuth => "windows",
            _ => "entra_id",
        };
        c.options.insert(AUTH.to_owned(), auth.to_owned());
        notes.push(note);
    }
    notes
}

/// DBeaver's authentication model (`sqlserver_native`, `sqlserver_ntlm`, …) and the values
/// of older versions' authentication properties, in lower case.
fn dbeaver_auth(config: &Value) -> String {
    let mut auth = text(config, "auth-model").unwrap_or_default();
    for (key, value) in config
        .get("provider-properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(k, _)| k.to_ascii_lowercase().contains("auth"))
    {
        auth.push(' ');
        match value {
            // A flag such as `@dbeaver-windows-auth@`: its name says which.
            Value::Bool(true) => auth.push_str(key),
            Value::String(v) if v == "true" => auth.push_str(key),
            Value::String(v) => auth.push_str(v),
            _ => {}
        }
    }
    auth.to_ascii_lowercase().replace('-', "_")
}

/// A SQL Server login other than a user and password: Windows' or Entra ID's.
fn sqlserver_login(auth: &str, props: &BTreeMap<String, String>) -> Option<ImportNote> {
    let prop = |k: &str| {
        props
            .get(k)
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_default()
    };
    let entra = [
        "_ad_",
        "ad_password",
        "ad_msi",
        "entra",
        "azure",
        "active_directory",
    ]
    .iter()
    .any(|w| auth.contains(w))
        || prop("authentication").starts_with("activedirectory");
    if entra {
        return Some(ImportNote::EntraAuth);
    }
    // `native` alone is DBeaver's generic user and password.
    let windows = auth.split_whitespace().any(|a| {
        a != "native"
            && ["native", "windows", "ntlm", "kerberos", "integrated"]
                .iter()
                .any(|w| a.contains(w))
    }) || prop("integratedsecurity") == "true"
        || matches!(
            prop("authenticationscheme").as_str(),
            "ntlm" | "javakerberos" | "nativeauthentication"
        )
        || !prop("domain").is_empty();
    windows.then_some(ImportNote::WindowsAuth)
}

fn pgadmin(json: &str) -> Result<ImportedProject, String> {
    let root: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let Some(servers) = root.get("Servers").and_then(Value::as_object) else {
        return Err("no \"Servers\": not a file exported by pgAdmin".to_owned());
    };
    let mut project = ImportedProject::new(Vec::new());
    let mut out: Vec<(u64, SavedConnection)> = Vec::new();
    for (key, s) in servers {
        let mut c = connection(
            &text(s, "Name").unwrap_or_else(|| "pgAdmin server".to_owned()),
            DriverKind::Postgres,
        );
        c.host = text(s, "Host")
            .or_else(|| text(s, "HostAddr"))
            .unwrap_or_default();
        c.port = text(s, "Port").and_then(|p| p.parse().ok()).or(Some(5432));
        c.database = text(s, "MaintenanceDB").unwrap_or_else(|| "postgres".to_owned());
        c.user = text(s, "Username").unwrap_or_default();
        if let Some(mode) = text(s, "SSLMode").and_then(|m| m.parse::<SslMode>().ok()) {
            c.ssl_mode = mode;
        }
        let tunnel = s.get("UseSSHTunnel").and_then(Value::as_i64).unwrap_or(0) == 1;
        if tunnel && let Some(host) = text(s, "TunnelHost") {
            let key = s.get("TunnelAuthentication").and_then(Value::as_i64) == Some(1);
            c.ssh = Some(SavedSsh {
                host,
                port: text(s, "TunnelPort")
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(22),
                user: text(s, "TunnelUsername").unwrap_or_default(),
                auth: if key {
                    SshAuthKind::Key
                } else {
                    SshAuthKind::Password
                },
                key_path: text(s, "TunnelIdentityFile").map(PathBuf::from),
                secret_id: Uuid::new_v4(),
                jump: None,
            });
        }
        // "Servers" is pgAdmin's default group, no folder.
        if let Some(group) = text(s, "Group").filter(|g| g != "Servers") {
            project.place(c.id, &group);
        }
        out.push((key.parse().unwrap_or(u64::MAX), c));
    }
    out.sort_by_key(|(k, _)| *k);
    project.connections = out.into_iter().map(|(_, c)| c).collect();
    Ok(project)
}

fn pg_service(ini: &str) -> Vec<SavedConnection> {
    let mut out: Vec<SavedConnection> = Vec::new();
    for line in ini.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            // Without `dbname` libpq connects to the database named like the user.
            let mut c = connection(name, DriverKind::Postgres);
            c.database.clear();
            out.push(c);
            continue;
        }
        let (Some(c), Some((key, value))) = (out.last_mut(), line.split_once('=')) else {
            continue;
        };
        let value = value.trim().to_owned();
        match key.trim() {
            "host" | "hostaddr" => c.host = value,
            "port" => c.port = value.parse().ok().or(c.port),
            "dbname" => c.database = value,
            "user" => c.user = value,
            "sslmode" => c.ssl_mode = value.parse().unwrap_or(c.ssl_mode),
            "sslrootcert" => c.ssl_root_cert = Some(PathBuf::from(value)),
            _ => {}
        }
    }
    for c in &mut out {
        // Without a user either, the service name is the best guess left.
        if c.database.is_empty() {
            c.database = if c.user.is_empty() {
                c.name.clone()
            } else {
                c.user.clone()
            };
        }
    }
    out
}

/// The text of `<tag>…</tag>` in `xml`, entities decoded.
fn element(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{tag}>"))? + start;
    Some(unescape(&xml[start..end]))
}

/// An attribute of the first tag of `xml`.
fn attribute(xml: &str, name: &str) -> Option<String> {
    let tag = &xml[..xml.find('>')?];
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(unescape(&tag[start..end]))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// `<data-source …>…</data-source>` blocks.
fn data_sources(xml: &str) -> impl Iterator<Item = &str> {
    xml.split("<data-source ")
        .skip(1)
        .map(|block| block.split("</data-source>").next().unwrap_or(block))
}

fn datagrip(xml: &str, local: &str) -> Vec<SavedConnection> {
    let users: Vec<(String, String)> = data_sources(local)
        .filter_map(|b| {
            Some((
                attribute(&format!(" {b}"), "uuid")?,
                element(b, "user-name")?,
            ))
        })
        .collect();
    let mut out = Vec::new();
    for block in data_sources(xml) {
        let head = format!(" {block}");
        let name = attribute(&head, "name").unwrap_or_else(|| "DataGrip connection".to_owned());
        let mut c = connection(&name, DriverKind::Postgres);
        let Some(url) = element(block, "jdbc-url").and_then(|url| read_jdbc(&url)) else {
            continue;
        };
        apply_jdbc(&mut c, &url);
        if let Some(uuid) = attribute(&head, "uuid")
            && let Some((_, user)) = users.iter().find(|(u, _)| *u == uuid)
        {
            user.clone_into(&mut c.user);
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_dbeaver() {
        let json = r#"{"folders": {}, "connections": {
            "postgres-jdbc-1": {"provider": "postgresql", "driver": "postgres-jdbc", "name": "Prod PG",
              "folder": "Work", "configuration": {"host": "db.example.com", "port": "6543",
              "database": "app", "url": "jdbc:postgresql://db.example.com:6543/app", "type": "prod",
              "user": "me", "provider-properties": {"@dbeaver-show-non-default-db@": "true"}, "handlers": {"ssh_tunnel": {"enabled": true, "properties":
              {"host": "bastion", "port": "2222", "user": "ops", "authType": "PUBLIC_KEY", "keyPath": "C:/k/id"}}}}},
            "sqlite-1": {"provider": "generic", "driver": "sqlite_jdbc", "name": "Local",
              "configuration": {"url": "jdbc:sqlite:C:/data/app.db"}},
            "db2-1": {"provider": "db2", "name": "Skip me", "configuration": {"url": "jdbc:db2://x:50000/y"}}
        }}"#;
        let project = dbeaver(json, &Credentials::Missing).unwrap();
        assert_eq!(
            project.skipped,
            [SkippedConnection {
                name: "Skip me".to_owned(),
                provider: "db2".to_owned(),
                driver: String::new(),
            }]
        );
        let found = project.connections;
        assert_eq!(found.len(), 2);
        let local = &found[0];
        assert_eq!(
            (local.driver, local.database.as_str()),
            (DriverKind::Sqlite, "C:/data/app.db")
        );
        let pg = &found[1];
        assert_eq!(
            (pg.host.as_str(), pg.port, pg.database.as_str()),
            ("db.example.com", Some(6543), "app")
        );
        assert!(pg.show_all_databases, "DBeaver's Show all databases");
        assert!(!local.show_all_databases);
        assert_eq!(pg.user, "me");
        assert_eq!(pg.connection_type, Some(ConnectionType::Prod));
        let ssh = pg.ssh.as_ref().unwrap();
        assert_eq!(
            (ssh.host.as_str(), ssh.port, ssh.auth),
            ("bastion", 2222, SshAuthKind::Key)
        );
        assert!(!pg.save_password);
    }

    #[test]
    fn reads_pgadmin_and_pg_service() {
        let json = r#"{"Servers": {"2": {"Name": "B", "Host": "b", "Port": 5433, "MaintenanceDB": "x",
            "Username": "u", "SSLMode": "require", "Group": "Team"},
            "1": {"Name": "A", "Host": "a", "Port": 5432, "Username": "v", "UseSSHTunnel": 1,
            "TunnelHost": "jump", "TunnelUsername": "j", "TunnelAuthentication": 0}}}"#;
        let found = pgadmin(json).unwrap().connections;
        assert_eq!(
            found.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["A", "B"]
        );
        assert_eq!(found[1].ssl_mode, SslMode::Require);
        assert_eq!(found[0].ssh.as_ref().unwrap().host, "jump");

        let ini = "# services\n[reports]\nhost=r.example.com\nport=5440\ndbname=rep\nuser=ro\n\n[local]\nhost=localhost\n";
        let services = pg_service(ini);
        assert_eq!(services.len(), 2);
        assert_eq!(
            (
                services[0].host.as_str(),
                services[0].port,
                services[0].database.as_str()
            ),
            ("r.example.com", Some(5440), "rep")
        );
        assert_eq!(services[1].database, "local");

        // Without `dbname` libpq connects to the database named like the user.
        let services = pg_service("[app]\nhost=h\nuser=carol\n[bare]\nhost=h2\n");
        assert_eq!(services[0].database, "carol");
        assert_eq!(services[1].database, "bare");
    }

    #[test]
    fn text_files_with_byte_order_mark() {
        let dir = std::env::temp_dir().join(format!("pdb-bom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("pg_service.conf");
        std::fs::write(
            &file,
            "\u{feff}[first]\nhost=a\ndbname=d\n[second]\nhost=b\n",
        )
        .unwrap();
        let found = import(ImportSource::PgService, &file).unwrap();
        assert_eq!(found[0].connections.len(), 2);
        assert_eq!(found[0].connections[0].name, "first");
        let file = dir.join("servers.json");
        std::fs::write(
            &file,
            "\u{feff}{\"Servers\": {\"1\": {\"Name\": \"A\", \"Host\": \"a\", \"Port\": 5432, \"Username\": \"v\"}}}",
        )
        .unwrap();
        let found = import(ImportSource::Pgadmin, &file).unwrap();
        assert_eq!(found[0].connections.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_datagrip() {
        let shared = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4"><component name="DataSourceManagerImpl" format="xml" multifile-model="true">
  <data-source source="LOCAL" name="shop@localhost" uuid="u-1">
    <driver-ref>mysql.8</driver-ref><jdbc-url>jdbc:mysql://localhost:3307/shop?useSSL=false</jdbc-url>
  </data-source>
  <data-source source="LOCAL" name="pg &amp; co" uuid="u-2">
    <jdbc-url>jdbc:postgresql://[::1]/postgres</jdbc-url>
  </data-source>
</component></project>"#;
        let local = r#"<project><component name="dataSourceStorageLocal">
  <data-source name="shop@localhost" uuid="u-1"><user-name>shopper</user-name></data-source>
</component></project>"#;
        let found = datagrip(shared, local);
        assert_eq!(found.len(), 2);
        assert_eq!(
            (found[0].driver, found[0].port, found[0].database.as_str()),
            (DriverKind::MySql, Some(3307), "shop")
        );
        assert_eq!(found[0].user, "shopper");
        assert_eq!(
            (found[1].name.as_str(), found[1].host.as_str()),
            ("pg & co", "::1")
        );
        assert_eq!(found[1].port, Some(5432));
    }

    fn path(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| (*p).to_owned()).collect()
    }

    const FOLDERED: &str = r#"{"folders": {"Prod": {}, "Prod/Replica": {"parent": "Prod"}, "Empty": {}},
        "connections": {
          "a": {"provider": "postgresql", "name": "replica", "folder": "Prod/Replica",
                "configuration": {"host": "r", "database": "app"}},
          "b": {"provider": "postgresql", "name": "local", "folder": "Dev/Local",
                "configuration": {"host": "localhost", "database": "app"}},
          "c": {"provider": "postgresql", "name": "root", "configuration": {"host": "x"}}}}"#;

    #[test]
    fn dbeaver_folders() {
        let project = dbeaver(FOLDERED, &Credentials::Missing).unwrap();
        assert_eq!(project.name, None);
        assert_eq!(
            project.folders,
            [
                path(&["Dev"]),
                path(&["Dev", "Local"]),
                path(&["Empty"]),
                path(&["Prod"]),
                path(&["Prod", "Replica"]),
            ]
        );
        let by_name = |n: &str| project.connections.iter().find(|c| c.name == n).unwrap().id;
        assert_eq!(
            project.placement.get(&by_name("replica")),
            Some(&path(&["Prod", "Replica"]))
        );
        assert_eq!(
            project.placement.get(&by_name("local")),
            Some(&path(&["Dev", "Local"]))
        );
        assert_eq!(project.placement.get(&by_name("root")), None);
    }

    /// Writes a `.dbp` like DBeaver's File › Export › DBeaver project.
    fn dbp(dir: &Path, meta: bool) -> PathBuf {
        use std::io::Write as _;
        let path = dir.join("work.dbp");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        if meta {
            zip.start_file("meta.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?><archive version="1"><projects>
                <project name="Interno" description=""/><project name="Cliente &amp; Rossi"/>
                <project name="Vuoto"/></projects></archive>"#,
            )
            .unwrap();
        }
        zip.start_file(
            "projects/Cliente & Rossi/.dbeaver/data-sources.json",
            options,
        )
        .unwrap();
        zip.write_all(FOLDERED.as_bytes()).unwrap();
        zip.start_file("projects/Interno/.dbeaver/data-sources.json", options)
            .unwrap();
        zip.write_all(
            br#"{"connections": {"crm": {"provider": "mysql", "name": "crm",
                "configuration": {"host": "db01", "database": "crm"}}}}"#,
        )
        .unwrap();
        zip.add_directory("projects/Vuoto/", options).unwrap();
        zip.start_file("projects/Interno/Scripts/a.sql", options)
            .unwrap();
        zip.write_all(b"select 1").unwrap();
        zip.finish().unwrap();
        path
    }

    #[test]
    fn relative_files_are_found_in_the_project_folder_or_noted() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("proj").join(".dbeaver");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(dir.path().join("proj").join("here.db"), b"").unwrap();
        let json = config.join("data-sources.json");
        std::fs::write(
            &json,
            r#"{"connections": {
              "a": {"provider": "generic", "driver": "sqlite_jdbc", "name": "a", "configuration": {"url": "jdbc:sqlite:here.db"}},
              "b": {"provider": "generic", "driver": "sqlite_jdbc", "name": "b", "configuration": {"url": "jdbc:sqlite:gone.db"}}}}"#,
        )
        .unwrap();
        let project = import(ImportSource::Dbeaver, &json).unwrap().remove(0);
        let (a, b) = (&project.connections[0], &project.connections[1]);
        assert_eq!(
            Path::new(&a.database),
            dir.path().join("proj").join("here.db")
        );
        assert!(!project.notes.contains_key(&a.id));
        assert_eq!(b.database, "gone.db");
        assert_eq!(
            project.notes[&b.id],
            [ImportNote::MissingFile {
                path: "gone.db".to_owned()
            }]
        );
    }

    #[test]
    fn dbp_two_projects() {
        let dir = tempfile::tempdir().unwrap();
        let projects = import(ImportSource::Dbeaver, &dbp(dir.path(), true)).unwrap();
        let names: Vec<_> = projects.iter().map(|p| p.name.as_deref()).collect();
        // In the order of meta.xml; a project without connections is left out.
        assert_eq!(names, [Some("Interno"), Some("Cliente & Rossi")]);
        assert_eq!(projects[0].connections.len(), 1);
        assert_eq!(projects[0].connections[0].driver, DriverKind::MySql);
        assert_eq!(projects[1].connections.len(), 3);
        assert_eq!(projects[1].folders.len(), 5);
    }

    #[test]
    fn dbp_reads_every_data_sources_file() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("more.dbp");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&file).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("projects/P/.dbeaver/data-sources.json", options)
            .unwrap();
        zip.write_all(FOLDERED.as_bytes()).unwrap();
        zip.start_file("projects/P/.dbeaver/data-sources-2.json", options)
            .unwrap();
        zip.write_all(
            br#"{"folders": {"Other": {}}, "connections": {"x": {"provider": "mysql",
                "name": "extra", "folder": "Other", "configuration": {"host": "h"}}}}"#,
        )
        .unwrap();
        zip.finish().unwrap();
        let projects = import(ImportSource::Dbeaver, &file).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].connections.len(), 4);
        assert!(projects[0].folders.contains(&path(&["Other"])));
        let extra = projects[0]
            .connections
            .iter()
            .find(|c| c.name == "extra")
            .unwrap();
        assert_eq!(
            projects[0].placement.get(&extra.id),
            Some(&path(&["Other"]))
        );
    }

    #[test]
    fn dbp_refuses_huge_entries() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.dbp");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("projects/P/.dbeaver/data-sources.json", options)
            .unwrap();
        // Compresses to almost nothing, unpacks past the limit.
        let chunk = vec![b' '; 1 << 20];
        for _ in 0..40 {
            zip.write_all(&chunk).unwrap();
        }
        zip.finish().unwrap();
        let err = import(ImportSource::Dbeaver, &path).unwrap_err();
        assert!(err.to_string().contains("too large"), "{err}");
    }

    #[test]
    fn dbp_without_meta() {
        let dir = tempfile::tempdir().unwrap();
        let projects = import(ImportSource::Dbeaver, &dbp(dir.path(), false)).unwrap();
        let names: Vec<_> = projects.iter().map(|p| p.name.as_deref()).collect();
        assert_eq!(names, [Some("Cliente & Rossi"), Some("Interno")]);
    }

    #[test]
    fn dbp_keeps_same_named_projects_apart() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("two.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (workspace, conn) in [("a", "one"), ("b", "two")] {
            zip.start_file(
                format!("{workspace}/General/.dbeaver/data-sources.json"),
                options,
            )
            .unwrap();
            zip.write_all(
                format!(
                    r#"{{"connections": {{"{conn}": {{"provider": "postgresql", "name": "{conn}",
                    "configuration": {{"host": "h", "database": "d"}}}}}}}}"#
                )
                .as_bytes(),
            )
            .unwrap();
        }
        zip.finish().unwrap();
        let projects = import(ImportSource::Dbeaver, &path).unwrap();
        let read: Vec<_> = projects
            .iter()
            .map(|p| (p.name.as_deref(), p.connections.len()))
            .collect();
        assert_eq!(read, [(Some("General"), 1), (Some("General"), 1)]);
    }

    #[test]
    fn dbeaver_source_accepts_json() {
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join("data-sources.json");
        std::fs::write(&json, FOLDERED).unwrap();
        let projects = import(ImportSource::Dbeaver, &json).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].connections.len(), 3);
    }

    #[test]
    fn pgadmin_groups_become_folders() {
        let json = r#"{"Servers": {"1": {"Name": "A", "Host": "a", "Group": "Team"},
            "2": {"Name": "B", "Host": "b", "Group": "Servers"}}}"#;
        let project = pgadmin(json).unwrap();
        assert_eq!(project.folders, [path(&["Team"])]);
        let a = project.connections[0].id;
        assert_eq!(project.placement.get(&a), Some(&path(&["Team"])));
        // pgAdmin's default group is no folder.
        assert_eq!(project.placement.len(), 1);
    }

    /// SQL Server as DBeaver writes it: by URL only, by fields, with Windows, NTLM or
    /// Entra ID logins, named instances, under the generic provider, and jTDS.
    const SQLSERVER: &str = r#"{"folders": {"Clienti": {}}, "connections": {
        "sqlserver-url": {"provider": "sqlserver", "driver": "mssql_jdbc_ms_new", "name": "Vericode",
          "folder": "Clienti", "configuration": {"configurationType": "URL",
          "url": "jdbc:sqlserver://vc-sql01:1433;databaseName=Vericode;encrypt=true;trustServerCertificate=true",
          "user": "sa", "auth-model": "sqlserver_database", "type": "prod"}},
        "sqlserver-fields": {"provider": "sqlserver", "driver": "microsoft", "name": "Fields",
          "configuration": {"host": "10.1.2.3", "port": "1500", "database": "erp",
          "auth-model": "sqlserver_database"}},
        "sqlserver-windows": {"provider": "sqlserver", "driver": "mssql_jdbc_ms_new", "name": "Windows",
          "configuration": {"host": "SRV\\SQLEXPRESS", "database": "app", "auth-model": "sqlserver_native"}},
        "sqlserver-instance-port": {"provider": "mssql", "driver": "microsoft", "name": "Instance with port",
          "configuration": {"url": "jdbc:sqlserver://SRV\\PROD:14330;databaseName=x;integratedSecurity=true"}},
        "generic-jtds": {"provider": "generic", "driver": "jtds_sqlserver", "name": "jTDS",
          "configuration": {"url": "jdbc:jtds:sqlserver://legacy:1433/erp;domain=CORP"}},
        "generic-ms": {"provider": "generic", "driver": "mssql_jdbc_ms", "name": "Generic",
          "configuration": {"host": "g", "properties": {"integratedSecurity": "true"}}},
        "azure": {"provider": "sqlserver", "driver": "azure_ms_new", "name": "Azure",
          "configuration": {"url": "jdbc:sqlserver://acme.database.windows.net:1433;database=sales;user=bob@acme.com",
          "auth-model": "sqlserver_ad_password"}},
        "ntlm": {"provider": "sqlserver", "driver": "mssql_jdbc_ms_new", "name": "NTLM",
          "configuration": {"host": "n", "auth-model": "sqlserver_ntlm"}},
        "sybase": {"provider": "generic", "driver": "jtds_sybase", "name": "Sybase",
          "configuration": {"url": "jdbc:jtds:sybase://s:5000/db"}}
    }}"#;

    fn named<'a>(project: &'a ImportedProject, name: &str) -> &'a SavedConnection {
        project.connections.iter().find(|c| c.name == name).unwrap()
    }

    fn notes_of(project: &ImportedProject, name: &str) -> Vec<ImportNote> {
        let id = named(project, name).id;
        project.notes.get(&id).cloned().unwrap_or_default()
    }

    fn address(c: &SavedConnection) -> (&str, Option<u16>, &str) {
        (c.host.as_str(), c.port, c.database.as_str())
    }

    #[test]
    fn dbeaver_sqlserver_shapes() {
        let project = dbeaver(SQLSERVER, &Credentials::Missing).unwrap();
        assert_eq!(project.connections.len(), 8);
        assert!(
            project
                .connections
                .iter()
                .all(|c| c.driver == DriverKind::SqlServer)
        );
        let at = |n: &str| address(named(&project, n));
        let vericode = named(&project, "Vericode");
        assert_eq!(at("Vericode"), ("vc-sql01", Some(1433), "Vericode"));
        assert_eq!(vericode.user, "sa");
        assert_eq!(vericode.connection_type, Some(ConnectionType::Prod));
        assert_eq!(
            project.placement.get(&vericode.id),
            Some(&path(&["Clienti"]))
        );
        assert_eq!(at("Fields"), ("10.1.2.3", Some(1500), "erp"));
        assert_eq!(at("jTDS"), ("legacy", Some(1433), "erp"));
        assert_eq!(at("Windows"), ("SRV", Some(1433), "app"));
        assert_eq!(at("Instance with port"), ("SRV", Some(14330), "x"));
        assert_eq!(named(&project, "Azure").user, "bob@acme.com");

        assert_eq!(notes_of(&project, "Vericode"), Vec::<ImportNote>::new());
        assert_eq!(notes_of(&project, "Fields"), Vec::<ImportNote>::new());
        assert_eq!(
            notes_of(&project, "Windows"),
            [
                ImportNote::NamedInstance {
                    instance: "SQLEXPRESS".to_owned()
                },
                ImportNote::WindowsAuth
            ]
        );
        // With a port the instance needs nothing.
        assert_eq!(
            notes_of(&project, "Instance with port"),
            [ImportNote::WindowsAuth]
        );
        assert_eq!(notes_of(&project, "jTDS"), [ImportNote::WindowsAuth]);
        assert_eq!(notes_of(&project, "Generic"), [ImportNote::WindowsAuth]);
        assert_eq!(notes_of(&project, "NTLM"), [ImportNote::WindowsAuth]);
        assert_eq!(notes_of(&project, "Azure"), [ImportNote::EntraAuth]);
        assert_eq!(named(&project, "NTLM").options[AUTH], "windows");
        assert_eq!(named(&project, "Azure").options[AUTH], "entra_id");
        assert!(!named(&project, "Fields").options.contains_key(AUTH));

        assert_eq!(
            project.skipped,
            [SkippedConnection {
                name: "Sybase".to_owned(),
                provider: "generic".to_owned(),
                driver: "jtds_sybase".to_owned(),
            }]
        );
    }

    #[test]
    fn dbeaver_older_windows_auth_flag() {
        let json = r#"{"connections": {"a": {"provider": "sqlserver", "name": "a",
            "configuration": {"host": "h", "provider-properties": {"@dbeaver-windows-auth@": "true",
            "@dbeaver-show-all-schemas@": "true"}}},
            "b": {"provider": "sqlserver", "name": "b",
            "configuration": {"host": "h", "provider-properties": {"@dbeaver-windows-auth@": "false"}}}}}"#;
        let project = dbeaver(json, &Credentials::Missing).unwrap();
        assert_eq!(notes_of(&project, "a"), [ImportNote::WindowsAuth]);
        assert_eq!(notes_of(&project, "b"), Vec::<ImportNote>::new());
    }

    #[test]
    fn dbeaver_json_with_byte_order_mark() {
        let json = format!("\u{feff}{SQLSERVER}");
        assert_eq!(
            dbeaver(&json, &Credentials::Missing)
                .unwrap()
                .connections
                .len(),
            8
        );
    }

    /// A zip with these entries.
    fn zip_of(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        use std::io::Write as _;
        let path = dir.join("export.dbp");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    const URL_ONLY: &[u8] = br#"{"connections": {"sqlserver-1": {"provider": "sqlserver",
        "driver": "mssql_jdbc_ms_new", "name": "Vericode", "configuration": {"configurationType": "URL",
        "url": "jdbc:sqlserver://vc-sql01:1433;databaseName=Vericode"}}}}"#;

    #[test]
    fn dbp_sqlserver_only_project() {
        let dir = tempfile::tempdir().unwrap();
        let meta = "<archive><projects><project name=\"Vericode Società\"/></projects></archive>";
        let file = zip_of(
            dir.path(),
            &[
                ("meta.xml", meta.as_bytes()),
                (
                    "projects/Vericode Società/.project",
                    b"<projectDescription/>",
                ),
                (
                    "projects/Vericode Società/.dbeaver/credentials-config.json",
                    b"\x01\x02encrypted",
                ),
                (
                    "projects/Vericode Società/.dbeaver/data-sources.json",
                    URL_ONLY,
                ),
            ],
        );
        let projects = import(ImportSource::Dbeaver, &file).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name.as_deref(), Some("Vericode Società"));
        let c = &projects[0].connections[0];
        assert_eq!(c.driver, DriverKind::SqlServer);
        assert_eq!(address(c), ("vc-sql01", Some(1433), "Vericode"));
        // Not DBeaver's encryption: imported without its logins, and said so.
        assert_eq!(
            projects[0].project_notes,
            [ProjectNote::SavedLoginsUnreadable]
        );
        assert!(projects[0].secrets.is_empty());
    }

    /// Connections by URL and by fields, both with leftovers of the other mode.
    const MODES: &[u8] = br#"{"connections": {
        "by-url": {"provider": "postgresql", "driver": "postgres-jdbc", "name": "by url",
          "configuration": {"configurationType": "URL", "host": "old-host", "port": "5999",
          "database": "old", "url": "jdbc:postgresql://new-host:6543/app"}},
        "by-fields": {"provider": "postgresql", "driver": "postgres-jdbc", "name": "by fields",
          "configuration": {"configurationType": "MANUAL", "host": "field-host", "port": "5440",
          "database": "field", "url": "jdbc:postgresql://stale:5432/stale"}},
        "sql-url": {"provider": "sqlserver", "driver": "mssql_jdbc_ms_new", "name": "sql url",
          "configuration": {"configurationType": "URL", "port": "1433",
          "url": "jdbc:sqlserver://SRV\\PROD;databaseName=x"}},
        "tunnel": {"provider": "mysql", "driver": "mysql8", "name": "tunnel",
          "configuration": {"host": "db", "database": "crm", "handlers": {"ssh_tunnel":
          {"enabled": true, "properties": {"host": "bastion", "authType": "PASSWORD"}}}}},
        "agent": {"provider": "mysql", "driver": "mysql8", "name": "agent",
          "configuration": {"host": "db", "handlers": {"ssh_tunnel":
          {"enabled": true, "properties": {"host": "bastion", "authType": "AGENT"}}}}}
    }}"#;

    #[test]
    fn dbeaver_url_or_fields() {
        let json = std::str::from_utf8(MODES).unwrap();
        let project = dbeaver(json, &Credentials::Missing).unwrap();
        assert_eq!(
            address(named(&project, "by url")),
            ("new-host", Some(6543), "app")
        );
        assert_eq!(
            address(named(&project, "by fields")),
            ("field-host", Some(5440), "field")
        );
        // The port field of a connection by URL does not count: the instance needs one.
        assert_eq!(
            notes_of(&project, "sql url"),
            [ImportNote::NamedInstance {
                instance: "PROD".to_owned()
            }]
        );
    }

    const LOGINS: &str = r##"{
        "by-url": {"#connection": {"user": "app", "password": "pg-pass"}},
        "by-fields": {"#connection": {"user": "reader"}},
        "tunnel": {"#connection": {"user": "crm", "password": "my-pass"},
                   "network/ssh_tunnel": {"user": "ops", "password": "ssh-pass"}},
        "agent": {"network/ssh_tunnel": {"user": "me", "password": "unused"}}}"##;

    fn encrypted(key: &[u8; 16]) -> Vec<u8> {
        credentials::tests::encrypt(LOGINS, key)
    }

    #[test]
    fn dbp_brings_saved_logins() {
        let dir = tempfile::tempdir().unwrap();
        let logins = encrypted(&credentials::DEFAULT_KEY);
        let file = zip_of(
            dir.path(),
            &[
                ("projects/P/.dbeaver/credentials-config.json", &logins),
                ("projects/P/.dbeaver/data-sources.json", MODES),
            ],
        );
        let project = &import(ImportSource::Dbeaver, &file).unwrap()[0];
        assert_eq!(project.project_notes, []);
        let by_url = named(project, "by url");
        assert_eq!(by_url.user, "app");
        assert!(!by_url.save_password);
        assert_eq!(named(project, "by fields").user, "reader");
        let tunnel = named(project, "tunnel");
        assert_eq!(tunnel.user, "crm");
        assert_eq!(tunnel.ssh.as_ref().unwrap().user, "ops");
        assert_eq!(
            project.secrets.get(&by_url.id),
            Some(&ImportedSecrets {
                password: Some(Secret::new("pg-pass")),
                ssh: None,
                jumps: Vec::new(),
            })
        );
        assert_eq!(
            project.secrets.get(&tunnel.id),
            Some(&ImportedSecrets {
                password: Some(Secret::new("my-pass")),
                ssh: Some(Secret::new("ssh-pass")),
                jumps: Vec::new(),
            })
        );
        // A user name alone, and an agent, which needs no password.
        let reader = named(project, "by fields").id;
        let agent = named(project, "agent");
        assert_eq!(agent.ssh.as_ref().unwrap().user, "me");
        assert_eq!(project.secrets.get(&reader), None);
        assert_eq!(project.secrets.get(&agent.id), None);
        let mut with_password = project.with_password.clone();
        with_password.sort();
        let mut expected = vec![by_url.id, tunnel.id];
        expected.sort();
        assert_eq!(with_password, expected);
        // What the dialog sees holds no password.
        let json = serde_json::to_string(project).unwrap();
        assert!(
            !json.contains("pg-pass") && !json.contains("ssh-pass"),
            "{json}"
        );
    }

    #[test]
    fn dbp_logins_under_another_key_or_missing() {
        let dir = tempfile::tempdir().unwrap();
        let logins = encrypted(&[9; 16]);
        let file = zip_of(
            dir.path(),
            &[
                ("projects/Locked/.dbeaver/credentials-config.json", &logins),
                ("projects/Locked/.dbeaver/data-sources.json", MODES),
                ("projects/None/.dbeaver/data-sources.json", MODES),
            ],
        );
        let projects = import(ImportSource::Dbeaver, &file).unwrap();
        let (locked, none) = (&projects[0], &projects[1]);
        assert_eq!(locked.project_notes, [ProjectNote::SavedLoginsUnreadable]);
        assert_eq!(none.project_notes, [ProjectNote::NoSavedLogins]);
        for p in &projects {
            assert!(p.secrets.is_empty() && p.with_password.is_empty());
            assert_eq!(named(p, "by url").user, "");
        }
    }

    #[test]
    fn workspace_logins_beside_data_sources() {
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join("data-sources.json");
        std::fs::write(&json, MODES).unwrap();
        let project = &import(ImportSource::Dbeaver, &json).unwrap()[0];
        assert_eq!(project.project_notes, [ProjectNote::NoSavedLogins]);
        std::fs::write(
            dir.path().join("credentials-config.json"),
            encrypted(&credentials::DEFAULT_KEY),
        )
        .unwrap();
        let project = &import(ImportSource::Dbeaver, &json).unwrap()[0];
        assert_eq!(project.project_notes, []);
        assert_eq!(named(project, "tunnel").user, "crm");
        assert_eq!(project.with_password.len(), 2);
    }

    #[test]
    fn dbeaver_clickhouse_duckdb_oracle_libsql_and_snowflake_are_imported() {
        let json = r#"{"connections": {
            "a": {"provider": "clickhouse", "driver": "com_clickhouse", "name": "ch",
              "configuration": {"host": "ch.example.com", "port": "8123", "database": "logs",
              "url": "jdbc:clickhouse://ch.example.com:8123/logs"}},
            "b": {"provider": "duckdb", "driver": "duckdb_jdbc", "name": "duck",
              "configuration": {"url": "jdbc:duckdb:C:/data/a.duckdb"}},
            "c": {"provider": "oracle", "driver": "oracle_thin", "name": "ora",
              "configuration": {"url": "jdbc:oracle:thin:@//db:1521/svc"}},
            "d": {"provider": "generic", "driver": "libsql", "name": "lib",
              "configuration": {"host": "x.turso.io", "port": "443"}},
            "e": {"provider": "snowflake", "driver": "snowflake_jdbc", "name": "snow",
              "configuration": {"host": "acme.snowflakecomputing.com"}}
        }}"#;
        let project = dbeaver(json, &Credentials::Missing).unwrap();
        assert_eq!(project.skipped, []);
        let drivers: Vec<_> = ["ch", "duck", "ora", "lib", "snow"]
            .iter()
            .map(|n| named(&project, n).driver)
            .collect();
        assert_eq!(
            drivers,
            [
                DriverKind::ClickHouse,
                DriverKind::DuckDb,
                DriverKind::Oracle,
                DriverKind::Libsql,
                DriverKind::Snowflake
            ]
        );
        let ch = named(&project, "ch");
        assert_eq!(
            (ch.host.as_str(), ch.port, ch.database.as_str()),
            ("ch.example.com", Some(8123), "logs")
        );
        assert_eq!(named(&project, "ora").database, "svc");
    }

    #[test]
    fn dbp_entry_paths() {
        let dir = tempfile::tempdir().unwrap();
        let other = br#"{"connections": {"o": {"provider": "db2", "driver": "db2_luw",
            "name": "Db2", "configuration": {"url": "jdbc:db2://o:50000/x"}}}}"#;
        let file = zip_of(
            dir.path(),
            &[
                // Written on Windows, and a zip of workspace folders without `projects/`.
                ("projects\\Win\\.dbeaver\\data-sources.json", URL_ONLY),
                ("workspace6/General/.dbeaver/data-sources-2.json", URL_ONLY),
                // Only a connection that is left out: the project stays, to say why.
                ("projects/Only Db2/.dbeaver/data-sources.json", other),
            ],
        );
        let projects = import(ImportSource::Dbeaver, &file).unwrap();
        let names: Vec<_> = projects.iter().map(|p| p.name.as_deref()).collect();
        assert_eq!(names, [Some("General"), Some("Only Db2"), Some("Win")]);
        assert_eq!(projects[0].connections.len(), 1);
        assert_eq!(projects[2].connections.len(), 1);
        assert_eq!(projects[1].connections.len(), 0);
        assert_eq!(projects[1].skipped[0].driver, "db2_luw");
    }

    #[test]
    fn dbp_without_connections_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let file = zip_of(dir.path(), &[("projects/P/Scripts/a.sql", b"select 1")]);
        let err = import(ImportSource::Dbeaver, &file).unwrap_err();
        assert!(err.to_string().contains("data-sources"), "{err}");
    }

    #[test]
    fn dbp_names_the_entry_that_is_not_json() {
        let dir = tempfile::tempdir().unwrap();
        let file = zip_of(
            dir.path(),
            &[("projects/P/.dbeaver/data-sources.json", b"{oops")],
        );
        let err = import(ImportSource::Dbeaver, &file).unwrap_err();
        assert!(
            err.to_string()
                .contains("projects/P/.dbeaver/data-sources.json"),
            "{err}"
        );
    }

    /// The export the E2E scenario imports.
    #[test]
    fn dbp_e2e_sqlserver_fixture() {
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/desktop/e2e/fixtures/sqlserver-url.dbp");
        let projects = import(ImportSource::Dbeaver, &file).unwrap();
        assert_eq!(projects.len(), 1);
        let project = &projects[0];
        assert_eq!(project.name.as_deref(), Some("Vericode Società"));
        let url = named(project, "sql-url");
        assert_eq!(address(url), ("vc-sql01", Some(1433), "Vericode"));
        // Its synthetic saved login, under DBeaver's default key.
        assert_eq!(url.user, "vc_reader");
        assert_eq!(project.with_password, [url.id]);
        assert_eq!(
            project.secrets[&url.id].password,
            Some(Secret::new("fake-password"))
        );
        assert_eq!(project.project_notes, []);
        assert_eq!(
            notes_of(project, "sql-windows"),
            [
                ImportNote::NamedInstance {
                    instance: "SQLEXPRESS".to_owned()
                },
                ImportNote::WindowsAuth
            ]
        );
        assert_eq!(project.skipped[0].name, "Legacy DB2");
    }
}
