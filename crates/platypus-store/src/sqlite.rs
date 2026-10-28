//! What the SQLite stores share: schema migrations tracked with `PRAGMA user_version`.

use rusqlite::Connection;

use crate::{Result, StoreError};

/// Applies the `migrations` not yet applied, in order; refuses a database from a newer app.
/// Each migration commits with its `user_version`, so a start killed halfway leaves either
/// both or neither. `error` wraps SQLite's errors in the store's own variant.
pub(crate) fn migrate(
    db: &Connection,
    migrations: &[&str],
    what: &str,
    error: fn(String) -> StoreError,
) -> Result<()> {
    let failed = |e: rusqlite::Error| error(e.to_string());
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(failed)?;
    let version = usize::try_from(version).unwrap_or(usize::MAX);
    if version > migrations.len() {
        return Err(error(format!(
            "{what} database is from a newer PlatypusDB (schema {version})"
        )));
    }
    for (i, migration) in migrations.iter().enumerate().skip(version) {
        let mut tx = db.unchecked_transaction().map_err(failed)?;
        if tx.execute_batch(migration).is_err() {
            // Earlier versions bumped `user_version` after the migration, not with it: a
            // start killed in between left it applied (in part) at the older version.
            tx.rollback().map_err(failed)?;
            tx = db.unchecked_transaction().map_err(failed)?;
            apply_again(&tx, migration).map_err(failed)?;
        }
        bump(&tx, i + 1).map_err(failed)?;
        tx.commit().map_err(failed)?;
    }
    Ok(())
}

fn bump(db: &Connection, version: usize) -> rusqlite::Result<()> {
    db.pragma_update(
        None,
        "user_version",
        i64::try_from(version).unwrap_or(i64::MAX),
    )
}

/// Runs `migration` one statement at a time, skipping the statements it already applied: a
/// column or an object that exists. The stores' migrations have no `;` inside a statement.
fn apply_again(db: &Connection, migration: &str) -> rusqlite::Result<()> {
    for statement in migration.split(';').filter(|s| !s.trim().is_empty()) {
        if let Err(e) = db.execute_batch(statement) {
            let message = e.to_string();
            if !(message.contains("duplicate column name") || message.contains("already exists")) {
                return Err(e);
            }
        }
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
    fn finishes_a_migration_applied_before_its_version() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE a (x INTEGER); ALTER TABLE a ADD COLUMN z TEXT;")
            .unwrap();
        let migrations = [
            "CREATE TABLE a (x INTEGER);",
            "ALTER TABLE a ADD COLUMN z TEXT; CREATE TABLE c (w INTEGER);",
        ];
        migrate(&db, &migrations, "test", StoreError::Catalog).unwrap();
        assert_eq!(version(&db), 2);
        db.execute("INSERT INTO c VALUES (1)", []).unwrap();
        // A migration that fails for another reason changes nothing, version included.
        let broken = Connection::open_in_memory().unwrap();
        let err = migrate(
            &broken,
            &["CREATE TABLE d (x INTEGER); INSERT INTO nope VALUES (1);"],
            "test",
            StoreError::Catalog,
        )
        .unwrap_err();
        assert!(matches!(err, StoreError::Catalog(m) if m.contains("nope")));
        assert_eq!(version(&broken), 0);
        assert!(broken.execute("INSERT INTO d VALUES (1)", []).is_err());
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
