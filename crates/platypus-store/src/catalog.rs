//! Notes and tags the user keeps on tables, views and columns of a saved connection, in a
//! local SQLite database: never written to the database they describe.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Result, StoreError};

const MIGRATIONS: &[&str] = &["CREATE TABLE notes (
        connection_id TEXT    NOT NULL,
        database      TEXT    NOT NULL,
        schema        TEXT    NOT NULL,
        object        TEXT    NOT NULL,
        column_name   TEXT    NOT NULL,
        note          TEXT    NOT NULL,
        updated_at    INTEGER NOT NULL, -- unix epoch milliseconds
        PRIMARY KEY (connection_id, database, schema, object, column_name)
    );
    CREATE TABLE tags (
        connection_id TEXT NOT NULL,
        database      TEXT NOT NULL,
        schema        TEXT NOT NULL,
        object        TEXT NOT NULL,
        column_name   TEXT NOT NULL,
        tag           TEXT NOT NULL,
        PRIMARY KEY (connection_id, database, schema, object, column_name, tag)
    );
    CREATE INDEX tags_by_tag ON tags (connection_id, tag);"];

/// What a note or tag is on. `database` is empty for the connection's own database, `schema`
/// where the database has none, `column` for the table or view itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CatalogKey {
    pub database: String,
    pub schema: String,
    pub object: String,
    pub column: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogEntry {
    pub key: CatalogKey,
    pub note: String,
    /// Lower case, without `#`, sorted.
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagCount {
    pub tag: String,
    pub count: u32,
}

#[derive(Debug)]
pub struct CatalogStore {
    db: Connection,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn db_error(e: rusqlite::Error) -> StoreError {
    StoreError::Catalog(e.to_string())
}

/// Tags as stored: trimmed, without a leading `#`, lower case, spaces as `-`, no duplicates.
#[must_use]
pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = tags
        .iter()
        .map(|t| {
            t.trim()
                .trim_start_matches('#')
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-")
                .to_lowercase()
        })
        .filter(|t| !t.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl CatalogStore {
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
        crate::sqlite::migrate(&db, MIGRATIONS, "catalog", StoreError::Catalog)?;
        Ok(Self { db })
    }

    /// Every note and tag of `connection`.
    pub fn entries(&self, connection: Uuid) -> Result<Vec<CatalogEntry>> {
        let id = connection.to_string();
        let mut out: BTreeMap<CatalogKey, CatalogEntry> = BTreeMap::new();
        let mut notes = self
            .db
            .prepare("SELECT database, schema, object, column_name, note FROM notes WHERE connection_id = ?1")
            .map_err(db_error)?;
        let rows = notes
            .query_map(params![id], |r| Ok((key_of(r)?, r.get::<_, String>(4)?)))
            .map_err(db_error)?;
        for row in rows {
            let (key, note) = row.map_err(db_error)?;
            out.insert(
                key.clone(),
                CatalogEntry {
                    key,
                    note,
                    tags: Vec::new(),
                },
            );
        }
        let mut tags = self
            .db
            .prepare("SELECT database, schema, object, column_name, tag FROM tags WHERE connection_id = ?1 ORDER BY tag")
            .map_err(db_error)?;
        let rows = tags
            .query_map(params![id], |r| Ok((key_of(r)?, r.get::<_, String>(4)?)))
            .map_err(db_error)?;
        for row in rows {
            let (key, tag) = row.map_err(db_error)?;
            out.entry(key.clone())
                .or_insert_with(|| CatalogEntry {
                    key,
                    note: String::new(),
                    tags: Vec::new(),
                })
                .tags
                .push(tag);
        }
        Ok(out.into_values().collect())
    }

    /// Replaces the note and tags of `key`; both empty delete it. Returns what was stored.
    pub fn save(
        &mut self,
        connection: Uuid,
        key: &CatalogKey,
        note: &str,
        tags: &[String],
    ) -> Result<CatalogEntry> {
        let id = connection.to_string();
        let tags = normalize_tags(tags);
        let note = note.trim();
        let tx = self.db.transaction().map_err(db_error)?;
        let k = params![id, key.database, key.schema, key.object, key.column];
        tx.execute(
            "DELETE FROM notes WHERE connection_id = ?1 AND database = ?2 AND schema = ?3 AND object = ?4 AND column_name = ?5",
            k,
        )
        .map_err(db_error)?;
        tx.execute(
            "DELETE FROM tags WHERE connection_id = ?1 AND database = ?2 AND schema = ?3 AND object = ?4 AND column_name = ?5",
            k,
        )
        .map_err(db_error)?;
        if !note.is_empty() {
            tx.execute(
                "INSERT INTO notes VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    key.database,
                    key.schema,
                    key.object,
                    key.column,
                    note,
                    now_ms()
                ],
            )
            .map_err(db_error)?;
        }
        for tag in &tags {
            tx.execute(
                "INSERT INTO tags VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, key.database, key.schema, key.object, key.column, tag],
            )
            .map_err(db_error)?;
        }
        tx.commit().map_err(db_error)?;
        Ok(CatalogEntry {
            key: key.clone(),
            note: note.to_owned(),
            tags,
        })
    }

    /// The tags of `connection`, the most used first.
    pub fn tags(&self, connection: Uuid) -> Result<Vec<TagCount>> {
        let mut q = self
            .db
            .prepare("SELECT tag, COUNT(*) FROM tags WHERE connection_id = ?1 GROUP BY tag ORDER BY COUNT(*) DESC, tag")
            .map_err(db_error)?;
        let rows = q
            .query_map(params![connection.to_string()], |r| {
                Ok(TagCount {
                    tag: r.get(0)?,
                    count: r.get(1)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(db_error)
    }

    /// Moves notes and tags after the app renamed an object (`from.column` empty: its
    /// columns' notes move with it) or a column.
    pub fn rename(&mut self, connection: Uuid, from: &CatalogKey, to: &CatalogKey) -> Result<()> {
        let id = connection.to_string();
        let tx = self.db.transaction().map_err(db_error)?;
        for table in ["notes", "tags"] {
            let sql = if from.column.is_empty() {
                format!(
                    "UPDATE OR REPLACE {table} SET schema = ?5, object = ?6
                     WHERE connection_id = ?1 AND database = ?2 AND schema = ?3 AND object = ?4"
                )
            } else {
                format!(
                    "UPDATE OR REPLACE {table} SET column_name = ?6
                     WHERE connection_id = ?1 AND database = ?2 AND schema = ?3 AND object = ?4 AND column_name = ?5"
                )
            };
            let p = if from.column.is_empty() {
                params![
                    id,
                    from.database,
                    from.schema,
                    from.object,
                    to.schema,
                    to.object
                ]
            } else {
                params![
                    id,
                    from.database,
                    from.schema,
                    from.object,
                    from.column,
                    to.column
                ]
            };
            tx.execute(&sql, p).map_err(db_error)?;
        }
        tx.commit().map_err(db_error)
    }

    /// Forgets everything about a deleted connection.
    pub fn remove_connection(&mut self, connection: Uuid) -> Result<()> {
        let id = connection.to_string();
        self.db
            .execute("DELETE FROM notes WHERE connection_id = ?1", params![id])
            .map_err(db_error)?;
        self.db
            .execute("DELETE FROM tags WHERE connection_id = ?1", params![id])
            .map_err(db_error)?;
        Ok(())
    }
}

fn key_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<CatalogKey> {
    Ok(CatalogKey {
        database: r.get(0)?,
        schema: r.get(1)?,
        object: r.get(2)?,
        column: r.get(3)?,
    })
}

#[cfg(test)]
#[allow(
    clippy::assert_is_empty,
    reason = "clearer than comparing with an empty array"
)]
mod tests {
    use super::*;

    fn key(object: &str, column: &str) -> CatalogKey {
        CatalogKey {
            database: String::new(),
            schema: "public".to_owned(),
            object: object.to_owned(),
            column: column.to_owned(),
        }
    }

    fn tags(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn saves_and_lists_notes_and_tags() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        let e = s
            .save(
                c,
                &key("orders", ""),
                "One row per order",
                &tags(&["#Sales", "core", "core"]),
            )
            .unwrap();
        assert_eq!(e.tags, ["core", "sales"]);
        s.save(c, &key("orders", "email"), "", &tags(&["pii"]))
            .unwrap();
        let mut all = s.entries(c).unwrap();
        all.sort_by(|a, b| a.key.cmp(&b.key));
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].note, "One row per order");
        assert_eq!(all[1].key.column, "email");
        assert_eq!(all[1].note, "");
        assert_eq!(all[1].tags, ["pii"]);
        // Other connections see nothing.
        assert!(s.entries(Uuid::new_v4()).unwrap().is_empty());
    }

    #[test]
    fn empty_entry_is_deleted() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        s.save(c, &key("orders", ""), "x", &tags(&["a"])).unwrap();
        s.save(c, &key("orders", ""), "  ", &[]).unwrap();
        assert!(s.entries(c).unwrap().is_empty());
    }

    #[test]
    fn counts_tags() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        s.save(c, &key("a", ""), "", &tags(&["pii", "core"]))
            .unwrap();
        s.save(c, &key("b", ""), "", &tags(&["pii"])).unwrap();
        let counts = s.tags(c).unwrap();
        assert_eq!(counts[0].tag, "pii");
        assert_eq!(counts[0].count, 2);
        assert_eq!(counts[1].tag, "core");
    }

    #[test]
    fn renames_a_table_with_its_columns() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        s.save(c, &key("orders", ""), "t", &[]).unwrap();
        s.save(c, &key("orders", "email"), "c", &tags(&["pii"]))
            .unwrap();
        s.rename(c, &key("orders", ""), &key("sales_orders", ""))
            .unwrap();
        let all = s.entries(c).unwrap();
        assert!(all.iter().all(|e| e.key.object == "sales_orders"));
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn renames_a_column() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        s.save(c, &key("orders", "email"), "c", &tags(&["pii"]))
            .unwrap();
        s.rename(c, &key("orders", "email"), &key("orders", "mail"))
            .unwrap();
        let all = s.entries(c).unwrap();
        assert_eq!(all[0].key.column, "mail");
        assert_eq!(all[0].tags, ["pii"]);
    }

    #[test]
    fn removes_a_connection() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        s.save(c, &key("orders", ""), "t", &tags(&["a"])).unwrap();
        s.remove_connection(c).unwrap();
        assert!(s.entries(c).unwrap().is_empty());
        assert!(s.tags(c).unwrap().is_empty());
    }

    #[test]
    fn reopens_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalog.db");
        let c = Uuid::new_v4();
        CatalogStore::open(&path)
            .unwrap()
            .save(c, &key("t", ""), "kept", &[])
            .unwrap();
        assert_eq!(
            CatalogStore::open(&path).unwrap().entries(c).unwrap()[0].note,
            "kept"
        );
    }

    #[test]
    fn normalizes_tags() {
        assert_eq!(
            normalize_tags(&tags(&[" #PII ", "two words", "", "#"])),
            ["pii", "two-words"]
        );
    }
}
