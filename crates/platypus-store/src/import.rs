//! Connections imported from other tools: DBeaver (`data-sources.json`, or the projects of
//! a `.dbp` export), DataGrip (`dataSources.xml`), pgAdmin (an exported `servers.json`) and
//! libpq's `pg_service.conf`. Folders come along; passwords do not: each connection asks for
//! its password on first use.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use platypus_core::{DriverKind, SslMode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::connections::{ConnectionType, SavedConnection, SavedSsh, SshAuthKind};
use crate::{Result, StoreError};

mod dbp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportSource {
    Dbeaver,
    Datagrip,
    Pgadmin,
    PgService,
}

/// A file found where a tool keeps its connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportFile {
    pub source: ImportSource,
    pub path: PathBuf,
}

/// What a file holds for one project: its connections and their folders. Files of a single
/// set of connections give one project without a name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedProject {
    pub name: Option<String>,
    /// Each folder as its names from the root; parents before their children.
    pub folders: Vec<Vec<String>>,
    pub connections: Vec<SavedConnection>,
    /// The folder of each connection not at the root.
    pub placement: BTreeMap<Uuid, Vec<String>>,
}

impl ImportedProject {
    fn new(connections: Vec<SavedConnection>) -> Self {
        Self {
            name: None,
            folders: Vec::new(),
            connections,
            placement: BTreeMap::new(),
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
/// a `data-sources.json` or a `.dbp` export with several projects.
pub fn import(source: ImportSource, path: &Path) -> Result<Vec<ImportedProject>> {
    let read = |p: &Path| {
        std::fs::read(p).map_err(|source| StoreError::Io {
            path: p.to_owned(),
            source,
        })
    };
    let read_text = |p: &Path| read(p).map(|b| String::from_utf8_lossy(&b).into_owned());
    let invalid = |message: String| StoreError::Format {
        path: path.to_owned(),
        message,
    };
    let one = |connections| Ok(vec![ImportedProject::new(connections)]);
    match source {
        ImportSource::Dbeaver => {
            let bytes = read(path)?;
            if bytes.starts_with(b"PK\x03\x04") {
                dbp::projects(&bytes).map_err(invalid)
            } else {
                dbeaver(&String::from_utf8_lossy(&bytes))
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
    }
}

fn connection(name: &str, driver: DriverKind) -> SavedConnection {
    let mut c = SavedConnection::new(name, driver);
    // No password comes along: ask for it on first use.
    c.save_password = false;
    c
}

/// Fills driver, host, port and database from a JDBC URL; `false` for other databases.
fn apply_jdbc(c: &mut SavedConnection, url: &str) -> bool {
    let Some(rest) = url.strip_prefix("jdbc:") else {
        return false;
    };
    if let Some(path) = rest.strip_prefix("sqlite:") {
        c.driver = DriverKind::Sqlite;
        c.host.clear();
        c.port = None;
        path.clone_into(&mut c.database);
        return true;
    }
    let (driver, rest) = if let Some(r) = rest.strip_prefix("postgresql://") {
        (DriverKind::Postgres, r)
    } else if let Some(r) = rest
        .strip_prefix("mysql://")
        .or_else(|| rest.strip_prefix("mariadb://"))
    {
        (DriverKind::MySql, r)
    } else {
        return false;
    };
    c.driver = driver;
    let rest = rest.split(['?', ';']).next().unwrap_or_default();
    let (address, database) = rest.split_once('/').unwrap_or((rest, ""));
    // `[::1]:5432`, `host:5432` or just a host.
    let (host, port) = match address.strip_prefix('[') {
        Some(inner) => match inner.split_once(']') {
            Some((h, rest)) => (h, rest.strip_prefix(':').and_then(|p| p.parse().ok())),
            None => (inner, None),
        },
        None => match address.rsplit_once(':') {
            Some((h, p)) => (h, p.parse().ok()),
            None => (address, None),
        },
    };
    host.clone_into(&mut c.host);
    c.port = port.or(driver.default_port());
    database.clone_into(&mut c.database);
    true
}

fn text(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn dbeaver(json: &str) -> Result<ImportedProject, String> {
    let root: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
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
    let mut out = Vec::new();
    for entry in connections.values() {
        let config = entry.get("configuration").unwrap_or(&Value::Null);
        let name = text(entry, "name").unwrap_or_else(|| "DBeaver connection".to_owned());
        let provider = text(entry, "provider").unwrap_or_default();
        let driver_id = text(entry, "driver").unwrap_or_default();
        let mut c = connection(&name, DriverKind::Postgres);
        let known = match text(config, "url") {
            Some(url) if apply_jdbc(&mut c, &url) => true,
            _ => {
                let driver = match (provider.as_str(), driver_id.as_str()) {
                    ("postgresql", _) => Some(DriverKind::Postgres),
                    ("mysql" | "mariadb", _) => Some(DriverKind::MySql),
                    (_, d) if d.contains("sqlite") => Some(DriverKind::Sqlite),
                    _ => None,
                };
                if let Some(driver) = driver {
                    c.driver = driver;
                    c.port = driver.default_port();
                }
                driver.is_some()
            }
        };
        if !known {
            continue;
        }
        if let Some(host) = text(config, "host") {
            c.host = host;
        }
        if let Some(port) = text(config, "port").and_then(|p| p.parse().ok()) {
            c.port = Some(port);
        }
        if let Some(database) = text(config, "database") {
            c.database = database;
        }
        if let Some(user) = text(config, "user") {
            c.user = user;
        }
        c.read_only = entry
            .get("read-only")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        c.connection_type = match text(config, "type").as_deref() {
            Some("dev") => Some(ConnectionType::Dev),
            Some("test") => Some(ConnectionType::Test),
            Some("prod") => Some(ConnectionType::Prod),
            _ => None,
        };
        let tunnel = config
            .get("handlers")
            .and_then(|h| h.get("ssh_tunnel"))
            .filter(|t| t.get("enabled").and_then(Value::as_bool).unwrap_or(false));
        if let Some(props) = tunnel.and_then(|t| t.get("properties")) {
            c.ssh = text(props, "host").map(|host| SavedSsh {
                host,
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
            });
        }
        if let Some(folder) = text(entry, "folder") {
            project.place(c.id, &folder);
        }
        out.push(c);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    project.connections = out;
    Ok(project)
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
            let mut c = connection(name, DriverKind::Postgres);
            name.clone_into(&mut c.database);
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
        if !element(block, "jdbc-url").is_some_and(|url| apply_jdbc(&mut c, &url)) {
            continue;
        }
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
              "user": "me", "handlers": {"ssh_tunnel": {"enabled": true, "properties":
              {"host": "bastion", "port": "2222", "user": "ops", "authType": "PUBLIC_KEY", "keyPath": "C:/k/id"}}}}},
            "sqlite-1": {"provider": "generic", "driver": "sqlite_jdbc", "name": "Local",
              "configuration": {"url": "jdbc:sqlite:C:/data/app.db"}},
            "oracle-1": {"provider": "oracle", "name": "Skip me", "configuration": {"url": "jdbc:oracle:thin:@x"}}
        }}"#;
        let found = dbeaver(json).unwrap().connections;
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
        let project = dbeaver(FOLDERED).unwrap();
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
}
