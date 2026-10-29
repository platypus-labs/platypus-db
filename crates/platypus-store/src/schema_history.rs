//! Snapshots of a schema's structure taken now and then, in a local SQLite database: the
//! timeline the "Schema history" tab shows and diffs. Never written to the database they
//! describe.
//!
//! The designs are kept as the JSON text the caller produced, so this crate needs no schema
//! dependency; it only hashes the text to tell that two snapshots are the same.

use std::fmt::Write as _;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Result, StoreError};

const MIGRATIONS: &[&str] = &["CREATE TABLE snapshots (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        connection_id TEXT    NOT NULL,
        database      TEXT,             -- NULL for the connection's own database
        schema        TEXT    NOT NULL,
        driver        TEXT    NOT NULL,
        taken_at      TEXT    NOT NULL, -- UTC, RFC 3339
        label         TEXT,
        how           TEXT    NOT NULL, -- 'manual' or 'scheduled'
        tables        INTEGER NOT NULL,
        hash          TEXT    NOT NULL,
        designs       TEXT    NOT NULL  -- JSON
    );
    CREATE INDEX snapshots_by_schema ON snapshots (connection_id, schema, id);"];

/// Who took a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotHow {
    Manual,
    Scheduled,
}

impl SnapshotHow {
    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Scheduled => "scheduled",
        }
    }

    fn parse(s: &str) -> Self {
        if s == "scheduled" {
            Self::Scheduled
        } else {
            Self::Manual
        }
    }
}

/// A snapshot without its designs, as the timeline lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotMeta {
    pub id: i64,
    pub connection_id: Uuid,
    pub database: Option<String>,
    pub schema: String,
    pub driver: String,
    /// UTC, RFC 3339.
    pub taken_at: String,
    pub label: Option<String>,
    pub how: SnapshotHow,
    pub tables: u32,
    pub hash: String,
}

/// A snapshot with the JSON of its table designs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub meta: SnapshotMeta,
    pub designs: String,
}

/// What to store. `designs` is the JSON array of the schema's table designs.
#[derive(Debug, Clone, Copy)]
pub struct NewSnapshot<'a> {
    pub connection: Uuid,
    pub database: Option<&'a str>,
    pub schema: &'a str,
    pub driver: &'a str,
    pub label: Option<&'a str>,
    pub how: SnapshotHow,
    pub tables: u32,
    pub designs: &'a str,
}

/// The result of taking a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotOutcome {
    /// Stored as a new snapshot.
    Taken(SnapshotMeta),
    /// Identical to the latest snapshot of the same schema, so nothing was stored; carries
    /// that latest snapshot.
    Unchanged(SnapshotMeta),
}

/// The schema history database.
///
/// # Errors
/// Every method that returns a `Result` fails with [`StoreError::Catalog`] when SQLite does
/// (a file that cannot be written, a corrupt database); `open` also fails with
/// [`StoreError::Io`] when the folder cannot be created, and with an error for a database
/// written by a newer app.
#[derive(Debug)]
pub struct SchemaHistoryStore {
    db: Connection,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn db_error(e: rusqlite::Error) -> StoreError {
    StoreError::Catalog(e.to_string())
}

/// The SHA-256 of the designs' JSON, in hex.
fn hash_of(designs: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, designs.as_bytes());
    digest.as_ref().iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

const META_COLUMNS: &str =
    "id, connection_id, database, schema, driver, taken_at, label, how, tables, hash";

fn meta_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<SnapshotMeta> {
    let connection: String = r.get(1)?;
    Ok(SnapshotMeta {
        id: r.get(0)?,
        connection_id: Uuid::parse_str(&connection).unwrap_or_default(),
        database: r.get(2)?,
        schema: r.get(3)?,
        driver: r.get(4)?,
        taken_at: r.get(5)?,
        label: r.get(6)?,
        how: SnapshotHow::parse(&r.get::<_, String>(7)?),
        tables: r.get(8)?,
        hash: r.get(9)?,
    })
}

impl SchemaHistoryStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| StoreError::Io {
                path: dir.to_owned(),
                source,
            })?;
        }
        let db = Connection::open(path).map_err(db_error)?;
        db.pragma_update(None, "journal_mode", "WAL")
            .map_err(db_error)?;
        Self::init(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory().map_err(db_error)?)
    }

    fn init(db: Connection) -> Result<Self> {
        crate::sqlite::migrate(&db, MIGRATIONS, "schema history", StoreError::Catalog)?;
        Ok(Self { db })
    }

    /// Stores a snapshot, unless the latest one of the same connection, database and schema
    /// has the same designs: then nothing is written and that one is returned as
    /// [`SnapshotOutcome::Unchanged`].
    pub fn take(&mut self, new: &NewSnapshot<'_>) -> Result<SnapshotOutcome> {
        let hash = hash_of(new.designs);
        let id = new.connection.to_string();
        let latest = self
            .db
            .query_row(
                &format!(
                    "SELECT {META_COLUMNS} FROM snapshots
                     WHERE connection_id = ?1 AND database IS ?2 AND schema = ?3
                     ORDER BY id DESC LIMIT 1"
                ),
                params![id, new.database, new.schema],
                meta_of,
            )
            .optional()
            .map_err(db_error)?;
        if let Some(latest) = latest.filter(|m| m.hash == hash) {
            return Ok(SnapshotOutcome::Unchanged(latest));
        }
        let taken_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let label = new.label.map(str::trim).filter(|l| !l.is_empty());
        self.db
            .execute(
                "INSERT INTO snapshots
                 (connection_id, database, schema, driver, taken_at, label, how, tables, hash, designs)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    id,
                    new.database,
                    new.schema,
                    new.driver,
                    taken_at,
                    label,
                    new.how.as_str(),
                    new.tables,
                    hash,
                    new.designs
                ],
            )
            .map_err(db_error)?;
        Ok(SnapshotOutcome::Taken(SnapshotMeta {
            id: self.db.last_insert_rowid(),
            connection_id: new.connection,
            database: new.database.map(str::to_owned),
            schema: new.schema.to_owned(),
            driver: new.driver.to_owned(),
            taken_at,
            label: label.map(str::to_owned),
            how: new.how,
            tables: new.tables,
            hash,
        }))
    }

    /// The snapshots of a schema, newest first, without their designs.
    pub fn list(
        &self,
        connection: Uuid,
        database: Option<&str>,
        schema: &str,
    ) -> Result<Vec<SnapshotMeta>> {
        let mut q = self
            .db
            .prepare(&format!(
                "SELECT {META_COLUMNS} FROM snapshots
                 WHERE connection_id = ?1 AND database IS ?2 AND schema = ?3
                 ORDER BY id DESC"
            ))
            .map_err(db_error)?;
        let rows = q
            .query_map(params![connection.to_string(), database, schema], meta_of)
            .map_err(db_error)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(db_error)
    }

    /// One snapshot with its designs.
    pub fn get(&self, id: i64) -> Result<Option<Snapshot>> {
        self.db
            .query_row(
                &format!("SELECT {META_COLUMNS}, designs FROM snapshots WHERE id = ?1"),
                params![id],
                |r| {
                    Ok(Snapshot {
                        meta: meta_of(r)?,
                        designs: r.get(10)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    /// Deletes one snapshot; `false` when there was none.
    pub fn delete(&mut self, id: i64) -> Result<bool> {
        let n = self
            .db
            .execute("DELETE FROM snapshots WHERE id = ?1", params![id])
            .map_err(db_error)?;
        Ok(n > 0)
    }

    /// Forgets the snapshots of a deleted connection.
    pub fn remove_connection(&mut self, connection: Uuid) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM snapshots WHERE connection_id = ?1",
                params![connection.to_string()],
            )
            .map_err(db_error)?;
        Ok(())
    }

    /// Forgets the snapshots of every connection not in `keep`.
    pub fn retain_connections(&mut self, keep: &[Uuid]) -> Result<()> {
        let keep: Vec<String> = keep.iter().map(Uuid::to_string).collect();
        let marks = vec!["?"; keep.len()].join(", ");
        self.db
            .execute(
                &format!("DELETE FROM snapshots WHERE connection_id NOT IN ({marks})"),
                rusqlite::params_from_iter(&keep),
            )
            .map_err(db_error)?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(
    clippy::assert_is_empty,
    reason = "clearer than comparing with an empty array"
)]
mod tests {
    use super::*;

    fn new<'a>(c: Uuid, schema: &'a str, designs: &'a str) -> NewSnapshot<'a> {
        NewSnapshot {
            connection: c,
            database: None,
            schema,
            driver: "sqlite",
            label: None,
            how: SnapshotHow::Manual,
            tables: 1,
            designs,
        }
    }

    fn taken(o: SnapshotOutcome) -> SnapshotMeta {
        match o {
            SnapshotOutcome::Taken(m) => m,
            SnapshotOutcome::Unchanged(m) => panic!("unchanged: {m:?}"),
        }
    }

    #[test]
    fn takes_lists_and_gets() {
        let mut s = SchemaHistoryStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        let mut first = new(c, "main", "[1]");
        first.label = Some("  before  ");
        let a = taken(s.take(&first).unwrap());
        assert_eq!(a.label.as_deref(), Some("before"));
        let b = taken(s.take(&new(c, "main", "[2]")).unwrap());
        let list = s.list(c, None, "main").unwrap();
        assert_eq!(list.iter().map(|m| m.id).collect::<Vec<_>>(), [b.id, a.id]);
        let got = s.get(a.id).unwrap().unwrap();
        assert_eq!(got.designs, "[1]");
        assert_eq!(got.meta, a);
        assert!(s.get(999).unwrap().is_none());
    }

    #[test]
    fn an_identical_snapshot_is_not_stored() {
        let mut s = SchemaHistoryStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        let a = taken(s.take(&new(c, "main", "[1]")).unwrap());
        let again = s.take(&new(c, "main", "[1]")).unwrap();
        assert_eq!(again, SnapshotOutcome::Unchanged(a));
        assert_eq!(s.list(c, None, "main").unwrap().len(), 1);
        // Only the latest counts: going back to an older state is a change.
        taken(s.take(&new(c, "main", "[2]")).unwrap());
        taken(s.take(&new(c, "main", "[1]")).unwrap());
        assert_eq!(s.list(c, None, "main").unwrap().len(), 3);
    }

    #[test]
    fn schemas_databases_and_connections_are_separate() {
        let mut s = SchemaHistoryStore::open_in_memory().unwrap();
        let (c, d) = (Uuid::new_v4(), Uuid::new_v4());
        taken(s.take(&new(c, "a", "[1]")).unwrap());
        taken(s.take(&new(c, "b", "[1]")).unwrap());
        taken(s.take(&new(d, "a", "[1]")).unwrap());
        let mut other_db = new(c, "a", "[1]");
        other_db.database = Some("shop");
        taken(s.take(&other_db).unwrap());
        assert_eq!(s.list(c, None, "a").unwrap().len(), 1);
        assert_eq!(s.list(c, Some("shop"), "a").unwrap().len(), 1);
        assert_eq!(s.list(c, Some("other"), "a").unwrap().len(), 0);
    }

    #[test]
    fn deletes_one_snapshot() {
        let mut s = SchemaHistoryStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        let a = taken(s.take(&new(c, "main", "[1]")).unwrap());
        assert!(s.delete(a.id).unwrap());
        assert!(!s.delete(a.id).unwrap());
        assert!(s.list(c, None, "main").unwrap().is_empty());
    }

    #[test]
    fn forgets_connections() {
        let mut s = SchemaHistoryStore::open_in_memory().unwrap();
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        for x in [a, b, c] {
            taken(s.take(&new(x, "main", "[1]")).unwrap());
        }
        s.remove_connection(a).unwrap();
        assert!(s.list(a, None, "main").unwrap().is_empty());
        s.retain_connections(&[b]).unwrap();
        assert_eq!(s.list(b, None, "main").unwrap().len(), 1);
        assert!(s.list(c, None, "main").unwrap().is_empty());
        s.retain_connections(&[]).unwrap();
        assert!(s.list(b, None, "main").unwrap().is_empty());
    }

    #[test]
    fn a_database_from_a_newer_app_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("schema-history.db");
        drop(SchemaHistoryStore::open(&path).unwrap());
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        assert!(SchemaHistoryStore::open(&path).is_err());
    }
}
