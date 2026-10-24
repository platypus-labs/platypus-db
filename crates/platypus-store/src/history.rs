use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Result, StoreError};

/// Schema migrations, applied in order and tracked with `PRAGMA user_version`.
const MIGRATIONS: &[&str] = &["CREATE TABLE history (
        id            INTEGER PRIMARY KEY,
        connection_id TEXT    NOT NULL,
        sql           TEXT    NOT NULL,
        started_at    INTEGER NOT NULL, -- unix epoch milliseconds
        duration_ms   INTEGER NOT NULL,
        rows          INTEGER,
        status        TEXT    NOT NULL,
        error         TEXT
    );
    CREATE INDEX history_connection ON history (connection_id, id);"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryStatus {
    Ok,
    Error,
    Cancelled,
}

impl HistoryStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "ok" => Self::Ok,
            "cancelled" => Self::Cancelled,
            _ => Self::Error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub connection_id: Uuid,
    pub sql: String,
    /// Unix epoch milliseconds.
    pub started_at: i64,
    pub duration_ms: u64,
    /// Rows returned or affected, when known.
    pub rows: Option<u64>,
    pub status: HistoryStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewHistoryEntry {
    pub connection_id: Uuid,
    pub sql: String,
    pub started_at: SystemTime,
    pub duration: Duration,
    pub rows: Option<u64>,
    pub status: HistoryStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct HistoryQuery {
    /// Entries of these connections only, e.g. a project's; `None` for all of them.
    pub connection_ids: Option<Vec<Uuid>>,
    /// Case-insensitive substring of the SQL.
    pub text: Option<String>,
    /// Page backwards from this entry (exclusive).
    pub before_id: Option<i64>,
    pub limit: usize,
}

/// Executed statements, newest first, in a local SQLite database.
#[derive(Debug)]
pub struct HistoryStore {
    db: Connection,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn db_error(e: rusqlite::Error) -> StoreError {
    StoreError::History(e.to_string())
}

fn clamp_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

impl HistoryStore {
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
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(db_error)?;
        let version = usize::try_from(version).unwrap_or(usize::MAX);
        if version > MIGRATIONS.len() {
            return Err(StoreError::History(format!(
                "history database is from a newer PlatypusDB (schema {version})"
            )));
        }
        for (i, migration) in MIGRATIONS.iter().enumerate().skip(version) {
            db.execute_batch(migration).map_err(db_error)?;
            db.pragma_update(None, "user_version", clamp_i64(i as u64 + 1))
                .map_err(db_error)?;
        }
        Ok(Self { db })
    }

    pub fn record(&self, entry: &NewHistoryEntry) -> Result<i64> {
        let started_at = entry
            .started_at
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        self.db
            .execute(
                "INSERT INTO history (connection_id, sql, started_at, duration_ms, rows, status, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    entry.connection_id.to_string(),
                    entry.sql,
                    started_at,
                    clamp_i64(u64::try_from(entry.duration.as_millis()).unwrap_or(u64::MAX)),
                    entry.rows.map(clamp_i64),
                    entry.status.as_str(),
                    entry.error,
                ],
            )
            .map_err(db_error)?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn search(&self, query: &HistoryQuery) -> Result<Vec<HistoryEntry>> {
        // The ids go as one JSON array: any number of them, in one cached statement.
        let ids = match &query.connection_ids {
            Some(ids) if ids.is_empty() => return Ok(Vec::new()),
            // A `Uuid` serializes as its hyphenated string, the form `record` stores.
            Some(ids) => Some(serde_json::to_string(ids).unwrap_or_default()),
            None => None,
        };
        // `\` escapes LIKE wildcards typed by the user.
        let pattern = query.text.as_ref().filter(|t| !t.is_empty()).map(|t| {
            format!(
                "%{}%",
                t.replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            )
        });
        let mut stmt = self
            .db
            .prepare_cached(
                "SELECT id, connection_id, sql, started_at, duration_ms, rows, status, error
                 FROM history
                 WHERE (?1 IS NULL OR connection_id IN (SELECT value FROM json_each(?1)))
                   AND (?2 IS NULL OR sql LIKE ?2 ESCAPE '\\')
                   AND (?3 IS NULL OR id < ?3)
                 ORDER BY id DESC
                 LIMIT ?4",
            )
            .map_err(db_error)?;
        let rows = stmt
            .query_map(
                params![
                    ids,
                    pattern,
                    query.before_id,
                    i64::try_from(query.limit).unwrap_or(i64::MAX),
                ],
                |r| {
                    let connection_id: String = r.get(1)?;
                    let status: String = r.get(6)?;
                    Ok(HistoryEntry {
                        id: r.get(0)?,
                        connection_id: Uuid::parse_str(&connection_id).unwrap_or_default(),
                        sql: r.get(2)?,
                        started_at: r.get(3)?,
                        duration_ms: r.get::<_, i64>(4)?.try_into().unwrap_or(0),
                        rows: r.get::<_, Option<i64>>(5)?.and_then(|n| n.try_into().ok()),
                        status: HistoryStatus::parse(&status),
                        error: r.get(7)?,
                    })
                },
            )
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.db
            .execute("DELETE FROM history WHERE id = ?1", [id])
            .map_err(db_error)?;
        Ok(())
    }

    /// Keeps the newest `keep` entries; returns how many were removed.
    pub fn prune(&self, keep: usize) -> Result<usize> {
        let cutoff: Option<i64> = self
            .db
            .query_row(
                "SELECT id FROM history ORDER BY id DESC LIMIT 1 OFFSET ?1",
                [i64::try_from(keep).unwrap_or(i64::MAX)],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_error)?;
        match cutoff {
            Some(id) => self
                .db
                .execute("DELETE FROM history WHERE id <= ?1", [id])
                .map_err(db_error),
            None => Ok(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(connection_id: Uuid, sql: &str, status: HistoryStatus) -> NewHistoryEntry {
        NewHistoryEntry {
            connection_id,
            sql: sql.to_owned(),
            started_at: UNIX_EPOCH + Duration::from_hours(500_000),
            duration: Duration::from_millis(42),
            rows: Some(7),
            status,
            error: (status == HistoryStatus::Error).then(|| "boom".to_owned()),
        }
    }

    #[test]
    fn filters_on_several_connections() {
        let h = HistoryStore::open_in_memory().unwrap();
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        for (id, sql) in [(a, "a1"), (b, "b1"), (c, "c1"), (a, "a2")] {
            h.record(&entry(id, sql, HistoryStatus::Ok)).unwrap();
        }
        let found = h
            .search(&HistoryQuery {
                connection_ids: Some(vec![a, c]),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            found.iter().map(|e| e.sql.as_str()).collect::<Vec<_>>(),
            ["a2", "c1", "a1"]
        );
    }

    #[test]
    fn empty_list_matches_nothing() {
        let h = HistoryStore::open_in_memory().unwrap();
        h.record(&entry(Uuid::new_v4(), "x", HistoryStatus::Ok))
            .unwrap();
        let query = |ids: Option<Vec<Uuid>>| HistoryQuery {
            connection_ids: ids,
            limit: 10,
            ..Default::default()
        };
        assert_eq!(h.search(&query(Some(Vec::new()))).unwrap().len(), 0);
        assert_eq!(h.search(&query(None)).unwrap().len(), 1);
    }

    #[test]
    fn records_and_searches_newest_first() {
        let h = HistoryStore::open_in_memory().unwrap();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        h.record(&entry(a, "select 1", HistoryStatus::Ok)).unwrap();
        h.record(&entry(b, "select 100% off_line", HistoryStatus::Ok))
            .unwrap();
        let last = h
            .record(&entry(a, "SELECT broken", HistoryStatus::Error))
            .unwrap();

        let all = h
            .search(&HistoryQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            all.iter().map(|e| e.sql.as_str()).collect::<Vec<_>>(),
            ["SELECT broken", "select 100% off_line", "select 1"]
        );
        assert_eq!(all[0].id, last);
        assert_eq!(all[0].status, HistoryStatus::Error);
        assert_eq!(all[0].error.as_deref(), Some("boom"));
        assert_eq!(all[0].started_at, 1_800_000_000_000);
        assert_eq!((all[0].duration_ms, all[0].rows), (42, Some(7)));

        let only_a = HistoryQuery {
            connection_ids: Some(vec![a]),
            limit: 10,
            ..Default::default()
        };
        assert_eq!(h.search(&only_a).unwrap().len(), 2);

        let text = |t: &str| HistoryQuery {
            text: Some(t.to_owned()),
            limit: 10,
            ..Default::default()
        };
        assert_eq!(
            h.search(&text("select")).unwrap().len(),
            3,
            "case-insensitive"
        );
        assert_eq!(h.search(&text("100%")).unwrap().len(), 1, "literal %");
        assert_eq!(h.search(&text("f_l")).unwrap().len(), 1, "literal _");
        assert_eq!(h.search(&text("0% o")).unwrap().len(), 1);
        assert_eq!(h.search(&text("1_0")).unwrap().len(), 0);

        let page = HistoryQuery {
            before_id: Some(last),
            limit: 1,
            ..Default::default()
        };
        assert_eq!(h.search(&page).unwrap()[0].sql, "select 100% off_line");
    }

    #[test]
    fn prunes_and_deletes() {
        let h = HistoryStore::open_in_memory().unwrap();
        let id = Uuid::new_v4();
        for i in 0..10 {
            h.record(&entry(id, &format!("select {i}"), HistoryStatus::Ok))
                .unwrap();
        }
        assert_eq!(h.prune(4).unwrap(), 6);
        let left = h
            .search(&HistoryQuery {
                limit: 100,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(left.len(), 4);
        assert_eq!(left[0].sql, "select 9");
        h.delete(left[0].id).unwrap();
        assert_eq!(
            h.search(&HistoryQuery {
                limit: 100,
                ..Default::default()
            })
            .unwrap()
            .len(),
            3
        );
        assert_eq!(h.prune(100).unwrap(), 0);
    }

    #[test]
    fn reopens_existing_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        let id = Uuid::new_v4();
        HistoryStore::open(&path)
            .unwrap()
            .record(&entry(id, "select 1", HistoryStatus::Cancelled))
            .unwrap();
        let h = HistoryStore::open(&path).unwrap();
        let all = h
            .search(&HistoryQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(all[0].status, HistoryStatus::Cancelled);
    }
}
