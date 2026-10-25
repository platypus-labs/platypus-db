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
