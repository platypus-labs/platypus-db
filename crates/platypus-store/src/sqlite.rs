//! What the SQLite stores share: schema migrations tracked with `PRAGMA user_version`.

use rusqlite::Connection;

use crate::{Result, StoreError};

/// Applies the `migrations` not yet applied, in order; refuses a database from a newer app.
/// `error` wraps SQLite's errors in the store's own variant.
pub(crate) fn migrate(
    db: &Connection,
    migrations: &[&str],
    what: &str,
    error: fn(String) -> StoreError,
) -> Result<()> {
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| error(e.to_string()))?;
    let version = usize::try_from(version).unwrap_or(usize::MAX);
    if version > migrations.len() {
        return Err(error(format!(
            "{what} database is from a newer PlatypusDB (schema {version})"
        )));
    }
    for (i, migration) in migrations.iter().enumerate().skip(version) {
        db.execute_batch(migration)
            .map_err(|e| error(e.to_string()))?;
        db.pragma_update(
            None,
            "user_version",
            i64::try_from(i + 1).unwrap_or(i64::MAX),
        )
        .map_err(|e| error(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIGRATIONS: &[&str] = &["CREATE TABLE a (x INTEGER);", "CREATE TABLE b (y INTEGER);"];

    fn version(db: &Connection) -> i64 {
        db.pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn applies_the_pending_migrations_once() {
        let db = Connection::open_in_memory().unwrap();
        migrate(&db, &MIGRATIONS[..1], "test", StoreError::Catalog).unwrap();
        assert_eq!(version(&db), 1);
        // Reopening with one more migration only runs the new one: `a` would fail twice.
        migrate(&db, MIGRATIONS, "test", StoreError::Catalog).unwrap();
        migrate(&db, MIGRATIONS, "test", StoreError::Catalog).unwrap();
        assert_eq!(version(&db), 2);
        db.execute("INSERT INTO b VALUES (1)", []).unwrap();
    }

    #[test]
    fn refuses_a_database_from_a_newer_app() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "user_version", 3).unwrap();
        let err = migrate(&db, MIGRATIONS, "test", StoreError::Catalog).unwrap_err();
        assert!(
            matches!(&err, StoreError::Catalog(m) if m.contains("newer PlatypusDB") && m.contains("test")),
            "{err:?}"
        );
        // Nothing was applied or downgraded.
        assert_eq!(version(&db), 3);
        assert!(db.execute("INSERT INTO a VALUES (1)", []).is_err());
    }
}
