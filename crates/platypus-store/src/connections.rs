use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use platypus_core::{ConnectionConfig, DriverKind, Secret, SslMode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Result, SecretStore, StoreError};

const FILE_VERSION: u32 = 1;

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
    pub folder: Option<String>,
    /// Logical keys the user set for tables without a primary key, so the data editor can
    /// identify their rows. Last: TOML writes arrays of tables after plain values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub virtual_keys: Vec<VirtualKey>,
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
            folder: None,
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

#[derive(Debug, Serialize, Deserialize)]
struct ConnectionsFile {
    version: u32,
    #[serde(default)]
    connections: Vec<SavedConnection>,
}

#[derive(Debug)]
pub struct ConnectionStore {
    path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    connections: Vec<SavedConnection>,
}

impl ConnectionStore {
    /// Loads `path`, or starts empty if it does not exist yet.
    pub fn open(path: impl Into<PathBuf>, secrets: Arc<dyn SecretStore>) -> Result<Self> {
        let path = path.into();
        let connections = match std::fs::read_to_string(&path) {
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
                file.connections
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        Ok(Self {
            path,
            secrets,
            connections,
        })
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
        let before = self.connections.len();
        self.connections.retain(|c| c.id != id);
        if self.connections.len() == before {
            return Err(StoreError::NotFound(id));
        }
        self.write()?;
        self.secrets.delete(id)
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
        let text = toml::to_string_pretty(&ConnectionsFile {
            version: FILE_VERSION,
            connections: self.connections.clone(),
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

        let (mut s, _) = store(&dir.path().join("other"));
        let mut conn = sample();
        conn.name = "  ".to_owned();
        assert!(matches!(
            s.save(conn, PasswordUpdate::Keep),
            Err(StoreError::Invalid(_))
        ));
    }
}
