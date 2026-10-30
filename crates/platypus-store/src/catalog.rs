//! Notes and tags the user keeps on tables, views and columns of a saved connection, in a
//! local SQLite database: never written to the database they describe.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Result, StoreError};

const MIGRATIONS: &[&str] = &[
    "CREATE TABLE notes (
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
    CREATE INDEX tags_by_tag ON tags (connection_id, tag);",
    "CREATE TABLE display_columns (
        connection_id TEXT NOT NULL,
        database      TEXT NOT NULL,
        schema        TEXT NOT NULL,
        object        TEXT NOT NULL,
        display       TEXT NOT NULL,
        PRIMARY KEY (connection_id, database, schema, object)
    );",
    "CREATE TABLE sensitive_columns (
        connection_id TEXT    NOT NULL,
        database      TEXT    NOT NULL,
        schema        TEXT    NOT NULL,
        object        TEXT    NOT NULL,
        column_name   TEXT    NOT NULL,
        sensitive     INTEGER NOT NULL, -- 1 sensitive, 0 not, whatever the rules say
        PRIMARY KEY (connection_id, database, schema, object, column_name)
    );",
];

/// Every table of the catalog, for what applies to all of them.
const TABLES: [&str; 4] = ["notes", "tags", "display_columns", "sensitive_columns"];

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

/// The column the user picked to describe the rows of a table, shown beside the foreign keys
/// that point to it. `key.column` is always empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DisplayColumn {
    pub key: CatalogKey,
    pub column: String,
}

/// A column the user said is sensitive (personal data, secrets) or is not, over what the
/// rules on names and types say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SensitiveColumn {
    pub key: CatalogKey,
    pub sensitive: bool,
}

/// The catalog database.
///
/// # Errors
/// Every method that returns a `Result` fails with [`StoreError::Catalog`] when SQLite does
/// (a file that cannot be written, a corrupt database); `open` also fails with
/// [`StoreError::Io`] when the folder cannot be created, and with a catalog error for a
/// database written by a newer app.
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

    /// The display columns the user picked on the tables of `connection`.
    pub fn display_columns(&self, connection: Uuid) -> Result<Vec<DisplayColumn>> {
        let mut q = self
            .db
            .prepare("SELECT database, schema, object, display FROM display_columns WHERE connection_id = ?1")
            .map_err(db_error)?;
        let rows = q
            .query_map(params![connection.to_string()], |r| {
                Ok(DisplayColumn {
                    key: CatalogKey {
                        database: r.get(0)?,
                        schema: r.get(1)?,
                        object: r.get(2)?,
                        column: String::new(),
                    },
                    column: r.get(3)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(db_error)
    }

    /// Sets the column that describes the rows of the table `key`; `None` goes back to the
    /// automatic choice.
    pub fn set_display_column(
        &mut self,
        connection: Uuid,
        key: &CatalogKey,
        column: Option<&str>,
    ) -> Result<()> {
        let id = connection.to_string();
        let tx = self.db.transaction().map_err(db_error)?;
        tx.execute(
            "DELETE FROM display_columns WHERE connection_id = ?1 AND database = ?2 AND schema = ?3 AND object = ?4",
            params![id, key.database, key.schema, key.object],
        )
        .map_err(db_error)?;
        if let Some(column) = column {
            tx.execute(
                "INSERT INTO display_columns VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, key.database, key.schema, key.object, column],
            )
            .map_err(db_error)?;
        }
        tx.commit().map_err(db_error)
    }

    /// The columns of `connection` the user marked sensitive or not.
    pub fn sensitive_columns(&self, connection: Uuid) -> Result<Vec<SensitiveColumn>> {
        let mut q = self
            .db
            .prepare("SELECT database, schema, object, column_name, sensitive FROM sensitive_columns WHERE connection_id = ?1")
            .map_err(db_error)?;
        let rows = q
            .query_map(params![connection.to_string()], |r| {
                Ok(SensitiveColumn {
                    key: key_of(r)?,
                    sensitive: r.get(4)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(db_error)
    }

    /// Marks the column `key` sensitive or not; `None` leaves it to the rules again.
    pub fn set_sensitive(
        &mut self,
        connection: Uuid,
        key: &CatalogKey,
        sensitive: Option<bool>,
    ) -> Result<()> {
        let k = params![
            connection.to_string(),
            key.database,
            key.schema,
            key.object,
            key.column
        ];
        match sensitive {
            Some(sensitive) => self.db.execute(
                "INSERT OR REPLACE INTO sensitive_columns VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    connection.to_string(),
                    key.database,
                    key.schema,
                    key.object,
                    key.column,
                    sensitive
                ],
            ),
            None => self.db.execute(
                "DELETE FROM sensitive_columns WHERE connection_id = ?1 AND database = ?2 AND schema = ?3 AND object = ?4 AND column_name = ?5",
                k,
            ),
        }
        .map_err(db_error)?;
        Ok(())
    }

    /// Moves notes, tags and display columns after the app renamed an object or a column.
    ///
    /// With `from.column` empty the object `from.object` becomes `to.schema.to.object` and the
    /// entries of its columns move with it; otherwise the column of `from.object` becomes
    /// `to.column`. The rest of `to` is ignored: a rename never changes the database, and a
    /// column stays in its table. An entry already at the target is overwritten by the renamed
    /// one: a note and a display column are replaced, tags are merged.
    pub fn rename(&mut self, connection: Uuid, from: &CatalogKey, to: &CatalogKey) -> Result<()> {
        let id = connection.to_string();
        let object = from.column.is_empty();
        let p = if object {
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
        let tx = self.db.transaction().map_err(db_error)?;
        for table in TABLES {
            let sql = match (object, table) {
                (true, _) => format!(
                    "UPDATE OR REPLACE {table} SET schema = ?5, object = ?6
                     WHERE connection_id = ?1 AND database = ?2 AND schema = ?3 AND object = ?4"
                ),
                (false, "display_columns") => "UPDATE display_columns SET display = ?6
                     WHERE connection_id = ?1 AND database = ?2 AND schema = ?3 AND object = ?4 AND display = ?5"
                    .to_owned(),
                (false, _) => format!(
                    "UPDATE OR REPLACE {table} SET column_name = ?6
                     WHERE connection_id = ?1 AND database = ?2 AND schema = ?3 AND object = ?4 AND column_name = ?5"
                ),
            };
            tx.execute(&sql, p).map_err(db_error)?;
        }
        tx.commit().map_err(db_error)
    }

    /// Forgets everything about a deleted connection.
    pub fn remove_connection(&mut self, connection: Uuid) -> Result<()> {
        let id = connection.to_string();
        for table in TABLES {
            self.db
                .execute(
                    &format!("DELETE FROM {table} WHERE connection_id = ?1"),
                    params![id],
                )
                .map_err(db_error)?;
        }
        Ok(())
    }

    /// Forgets the catalog of every connection not in `keep`, in one transaction.
    pub fn retain_connections(&mut self, keep: &[Uuid]) -> Result<()> {
        let keep: Vec<String> = keep.iter().map(Uuid::to_string).collect();
        let marks = vec!["?"; keep.len()].join(", ");
        let tx = self.db.transaction().map_err(db_error)?;
        for table in TABLES {
            tx.execute(
                &format!("DELETE FROM {table} WHERE connection_id NOT IN ({marks})"),
                rusqlite::params_from_iter(&keep),
            )
            .map_err(db_error)?;
        }
        tx.commit().map_err(db_error)
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
    fn a_rename_onto_an_existing_key_overwrites_its_note_and_merges_tags() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        s.save(c, &key("orders", "email"), "old", &tags(&["pii"]))
            .unwrap();
        s.save(c, &key("orders", "mail"), "target", &tags(&["core"]))
            .unwrap();
        s.rename(c, &key("orders", "email"), &key("orders", "mail"))
            .unwrap();
        let all = s.entries(c).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].key, key("orders", "mail"));
        assert_eq!(all[0].note, "old");
        assert_eq!(all[0].tags, ["core", "pii"]);

        s.set_display_column(c, &key("customers", ""), Some("company"))
            .unwrap();
        s.set_display_column(c, &key("clients", ""), Some("name"))
            .unwrap();
        s.rename(c, &key("customers", ""), &key("clients", ""))
            .unwrap();
        let shown = s.display_columns(c).unwrap();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].key.object, "clients");
        assert_eq!(shown[0].column, "company");
    }

    #[test]
    fn a_rename_leaves_other_connections_and_objects_alone() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        for c in [a, b] {
            s.save(c, &key("orders", "email"), "mine", &tags(&["pii"]))
                .unwrap();
            s.save(c, &key("customers", "email"), "theirs", &[])
                .unwrap();
            s.set_display_column(c, &key("orders", ""), Some("email"))
                .unwrap();
        }
        s.rename(a, &key("orders", "email"), &key("orders", "mail"))
            .unwrap();
        s.rename(a, &key("orders", ""), &key("sales_orders", ""))
            .unwrap();

        let mut mine: Vec<_> = s
            .entries(a)
            .unwrap()
            .into_iter()
            .map(|e| (e.key.object, e.key.column))
            .collect();
        mine.sort();
        assert_eq!(
            mine,
            [
                ("customers".to_owned(), "email".to_owned()),
                ("sales_orders".to_owned(), "mail".to_owned())
            ]
        );
        let shown = s.display_columns(a).unwrap();
        assert_eq!(
            (shown[0].key.object.as_str(), shown[0].column.as_str()),
            ("sales_orders", "mail")
        );

        let untouched: Vec<_> = s
            .entries(b)
            .unwrap()
            .into_iter()
            .map(|e| (e.key.object, e.key.column))
            .collect();
        assert_eq!(
            untouched,
            [
                ("customers".to_owned(), "email".to_owned()),
                ("orders".to_owned(), "email".to_owned())
            ]
        );
        let shown = s.display_columns(b).unwrap();
        assert_eq!(
            (shown[0].key.object.as_str(), shown[0].column.as_str()),
            ("orders", "email")
        );
    }

    #[test]
    fn the_database_is_part_of_the_key() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        let main = key("orders", "");
        let other = CatalogKey {
            database: "analytics".to_owned(),
            ..main.clone()
        };
        s.save(c, &main, "main", &[]).unwrap();
        s.save(c, &other, "analytics", &[]).unwrap();
        assert_eq!(s.entries(c).unwrap().len(), 2);

        // A rename stays in the database of `from`, whatever `to` says.
        let moved = CatalogKey {
            object: "sales_orders".to_owned(),
            database: "elsewhere".to_owned(),
            ..other.clone()
        };
        s.rename(c, &other, &moved).unwrap();
        let all = s.entries(c).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|e| e.key == main && e.note == "main"));
        assert!(all.iter().any(|e| e.key.database == "analytics"
            && e.key.object == "sales_orders"
            && e.note == "analytics"));

        // Emptying one leaves the other.
        s.save(c, &main, "", &[]).unwrap();
        let all = s.entries(c).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].key.database, "analytics");
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

    #[test]
    fn retain_connections_drops_the_others() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        for c in [a, b] {
            s.save(c, &key("orders", ""), "note", &tags(&["core"]))
                .unwrap();
        }
        s.retain_connections(&[a]).unwrap();
        assert_eq!(s.entries(a).unwrap().len(), 1);
        assert_eq!(s.tags(a).unwrap().len(), 1);
        assert!(s.entries(b).unwrap().is_empty());
        assert!(s.tags(b).unwrap().is_empty());
    }

    #[test]
    fn saves_display_columns() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        s.set_display_column(c, &key("customers", ""), Some("company"))
            .unwrap();
        let all = s.display_columns(c).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].key.object, "customers");
        assert_eq!(all[0].column, "company");
        s.set_display_column(c, &key("customers", ""), None)
            .unwrap();
        assert_eq!(s.display_columns(c).unwrap(), Vec::new());
    }

    #[test]
    fn display_columns_follow_renames_and_removals() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        s.set_display_column(c, &key("customers", ""), Some("company"))
            .unwrap();
        s.rename(c, &key("customers", "company"), &key("customers", "firm"))
            .unwrap();
        assert_eq!(s.display_columns(c).unwrap()[0].column, "firm");
        s.rename(c, &key("customers", ""), &key("clients", ""))
            .unwrap();
        assert_eq!(s.display_columns(c).unwrap()[0].key.object, "clients");
        s.retain_connections(&[]).unwrap();
        assert_eq!(s.display_columns(c).unwrap(), Vec::new());
    }

    #[test]
    fn marks_columns_sensitive_or_not_and_follows_renames() {
        let mut s = CatalogStore::open_in_memory().unwrap();
        let c = Uuid::new_v4();
        s.set_sensitive(c, &key("users", "email"), Some(true))
            .unwrap();
        s.set_sensitive(c, &key("users", "phone_model"), Some(false))
            .unwrap();
        s.set_sensitive(c, &key("users", "email"), Some(true))
            .unwrap();
        let mut all = s.sensitive_columns(c).unwrap();
        all.sort_by(|a, b| a.key.cmp(&b.key));
        assert_eq!(
            all.iter()
                .map(|m| (m.key.column.as_str(), m.sensitive))
                .collect::<Vec<_>>(),
            [("email", true), ("phone_model", false)]
        );
        // A column renamed by the app keeps its mark; back to the rules forgets it.
        s.rename(c, &key("users", "email"), &key("users", "mail"))
            .unwrap();
        assert!(
            s.sensitive_columns(c)
                .unwrap()
                .iter()
                .any(|m| m.key.column == "mail" && m.sensitive)
        );
        s.set_sensitive(c, &key("users", "mail"), None).unwrap();
        assert_eq!(s.sensitive_columns(c).unwrap().len(), 1);
        assert!(s.sensitive_columns(Uuid::new_v4()).unwrap().is_empty());
        s.remove_connection(c).unwrap();
        assert!(s.sensitive_columns(c).unwrap().is_empty());
    }
}
