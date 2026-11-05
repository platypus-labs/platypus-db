//! The schema history: snapshots of a schema's structure, kept in `schema-history.db`, and
//! the changes between two of them or between one and the live schema (see
//! `platypus_store::SchemaHistoryStore` and `platypus_schema::summary`).

use platypus_core::{Connection, DriverKind};
use platypus_schema::{SchemaChange, TableDesign};
use platypus_sql::Dialect;
use platypus_store::{NewSnapshot, SnapshotHow, SnapshotMeta, SnapshotOutcome};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use super::AppState;
use crate::error::{CommandError, CommandResult};

/// The schema a snapshot is of: a saved connection, its database (`None` or empty for the
/// connection's own) and the schema.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SchemaRef<'a> {
    pub connection: Uuid,
    pub database: Option<&'a str>,
    pub schema: &'a str,
}

/// What `take_schema_snapshot` did.
#[derive(Debug, Serialize)]
pub struct SnapshotTaken {
    /// The same as the latest snapshot, so nothing was stored.
    unchanged: bool,
    /// The new snapshot, or the latest one when `unchanged`.
    snapshot: SnapshotMeta,
}

/// The name a driver is stored under: its serialized form (`postgres`, `sqlite`).
fn driver_name(driver: DriverKind) -> String {
    serde_json::to_value(driver)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn driver_of(name: &str) -> CommandResult<DriverKind> {
    serde_json::from_value(serde_json::Value::String(name.to_owned())).map_err(|_| {
        CommandError::new(
            "invalid",
            format!("the snapshot is of an unknown database: {name}"),
        )
    })
}

fn designs_of(json: &str) -> CommandResult<Vec<TableDesign>> {
    serde_json::from_str(json)
        .map_err(|e| CommandError::new("invalid", format!("the snapshot cannot be read: {e}")))
}

/// Reads the structure of `at` through `conn` and stores it as a snapshot, unless it is the
/// same as the latest one.
pub(crate) async fn take(
    state: &AppState,
    conn: &dyn Connection,
    driver: DriverKind,
    at: SchemaRef<'_>,
    label: Option<&str>,
    how: SnapshotHow,
) -> CommandResult<SnapshotOutcome> {
    let designs = platypus_schema::load_designs(conn, Dialect::from(driver), at.schema).await?;
    let json =
        serde_json::to_string(&designs).map_err(|e| CommandError::new("invalid", e.to_string()))?;
    let tables = u32::try_from(designs.len()).unwrap_or(u32::MAX);
    let driver = driver_name(driver);
    let outcome = state.with_schema_history(|h| {
        h.take(&NewSnapshot {
            connection: at.connection,
            database: at.database.filter(|d| !d.is_empty()),
            schema: at.schema,
            driver: &driver,
            label,
            how,
            tables,
            designs: &json,
        })
    })?;
    if matches!(outcome, SnapshotOutcome::Taken(_)) {
        note_change(at.connection);
    }
    Ok(outcome)
}

/// Tells the MCP servers subscribed to the schema of `connection` that it changed (see
/// `platypus_mcp::watch`). Best effort: a profile folder that cannot be written is not an
/// error of what the user did.
fn note_change(connection: Uuid) {
    if let Ok(profile) = platypus_mcp::Profile::from_env() {
        let _ = platypus_mcp::watch::note_change(profile.data_dir(), connection);
    }
}

/// The navigator refreshed the schema of `connection_id`: agents that subscribed to it are
/// told.
#[tauri::command]
pub fn note_schema_change(connection_id: Uuid) {
    note_change(connection_id);
}

/// Takes a snapshot of `schema` through the session, as the user asked.
#[tauri::command]
pub async fn take_schema_snapshot(
    state: State<'_, AppState>,
    session_id: u64,
    connection_id: Uuid,
    database: Option<String>,
    schema: String,
    label: Option<String>,
) -> CommandResult<SnapshotTaken> {
    let driver = state.session(session_id)?.config.driver;
    let conn = state.live(session_id).await?;
    let at = SchemaRef {
        connection: connection_id,
        database: database.as_deref(),
        schema: &schema,
    };
    let outcome = take(
        &state,
        conn.as_ref(),
        driver,
        at,
        label.as_deref(),
        SnapshotHow::Manual,
    )
    .await?;
    Ok(match outcome {
        SnapshotOutcome::Taken(snapshot) => SnapshotTaken {
            unchanged: false,
            snapshot,
        },
        SnapshotOutcome::Unchanged(snapshot) => SnapshotTaken {
            unchanged: true,
            snapshot,
        },
    })
}

/// The snapshots of `schema`, newest first.
#[tauri::command]
pub fn list_schema_snapshots(
    state: State<'_, AppState>,
    connection_id: Uuid,
    database: Option<String>,
    schema: String,
) -> CommandResult<Vec<SnapshotMeta>> {
    state.with_schema_history(|h| {
        h.list(
            connection_id,
            database.as_deref().filter(|d| !d.is_empty()),
            &schema,
        )
    })
}

/// Deletes a snapshot.
#[tauri::command]
pub fn delete_schema_snapshot(state: State<'_, AppState>, id: i64) -> CommandResult<()> {
    state.with_schema_history(|h| h.delete(id).map(|_| ()))
}

/// What a snapshot is compared with.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiffTarget {
    /// Another snapshot.
    Snapshot { id: i64 },
    /// The schema now, read through the session.
    Live { session_id: u64 },
}

/// The changes from one state of a schema to another, and the script that makes them.
#[derive(Debug, Serialize)]
pub struct SchemaDiff {
    changes: Vec<SchemaChange>,
    /// The script that turns the first state into the second.
    sql: String,
    /// What the script cannot do or what it costs.
    warnings: Vec<String>,
}

/// The changes and the script that turn `old` into `new`, in `schema`.
fn diff(
    dialect: Dialect,
    schema: &str,
    old: &[TableDesign],
    new: &[TableDesign],
) -> CommandResult<SchemaDiff> {
    let script = platypus_schema::compare(dialect, schema, schema, new, old)
        .map_err(|e| CommandError::new("invalid_design", e))?;
    Ok(SchemaDiff {
        changes: platypus_schema::summary(old, new),
        sql: script.text(dialect),
        warnings: script.warnings,
    })
}

/// The changes from the snapshot `from` to another snapshot or to the live schema (the
/// snapshot's own schema), and the script for them.
#[tauri::command]
pub async fn diff_schema_snapshots(
    state: State<'_, AppState>,
    from_id: i64,
    to: DiffTarget,
) -> CommandResult<SchemaDiff> {
    let from = snapshot(&state, from_id)?;
    let driver = driver_of(&from.meta.driver)?;
    let dialect = Dialect::from(driver);
    let schema = &from.meta.schema;
    let old = designs_of(&from.designs)?;
    let new = match to {
        DiffTarget::Snapshot { id } => {
            let to = snapshot(&state, id)?;
            if (to.meta.connection_id, &to.meta.database, &to.meta.schema)
                != (
                    from.meta.connection_id,
                    &from.meta.database,
                    &from.meta.schema,
                )
            {
                return Err(CommandError::new(
                    "invalid",
                    "The snapshots are of different schemas.",
                ));
            }
            designs_of(&to.designs)?
        }
        DiffTarget::Live { session_id } => {
            if state.session(session_id)?.config.driver != driver {
                return Err(CommandError::new(
                    "different_databases",
                    "The snapshot is of another kind of database than the connection.",
                ));
            }
            let conn = state.live(session_id).await?;
            platypus_schema::load_designs(conn.as_ref(), dialect, schema).await?
        }
    };
    diff(dialect, schema, &old, &new)
}

fn snapshot(state: &AppState, id: i64) -> CommandResult<platypus_store::Snapshot> {
    state
        .with_schema_history(|h| h.get(id))?
        .ok_or_else(|| CommandError::new("not_found", "That snapshot no longer exists."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use platypus_schema::{ChangeKind, ColumnDesign};

    fn table(name: &str, columns: &[(&str, &str)]) -> TableDesign {
        TableDesign {
            name: name.to_owned(),
            columns: columns
                .iter()
                .map(|(n, t)| ColumnDesign {
                    original: Some((*n).to_owned()),
                    name: (*n).to_owned(),
                    data_type: (*t).to_owned(),
                    nullable: true,
                    ..ColumnDesign::default()
                })
                .collect(),
            ..TableDesign::default()
        }
    }

    #[test]
    fn drivers_round_trip_through_their_stored_name() {
        for driver in [DriverKind::Sqlite, DriverKind::Postgres, DriverKind::MySql] {
            assert_eq!(driver_of(&driver_name(driver)).unwrap(), driver);
        }
        assert!(driver_of("nope").is_err());
    }

    #[test]
    fn a_diff_has_the_changes_and_the_script_from_old_to_new() {
        let old = [table("a", &[("id", "INTEGER")])];
        let new = [
            table("a", &[("id", "INTEGER"), ("name", "TEXT")]),
            table("b", &[("id", "INTEGER")]),
        ];
        let d = diff(Dialect::Sqlite, "main", &old, &new).unwrap();
        let kinds: Vec<_> = d.changes.iter().map(|c| c.kind).collect();
        assert_eq!(kinds, [ChangeKind::ColumnAdded, ChangeKind::TableAdded]);
        assert!(d.sql.contains("ADD COLUMN"), "{}", d.sql);
        assert!(d.sql.contains("CREATE TABLE"), "{}", d.sql);
        // The other way round drops the table.
        let back = diff(Dialect::Sqlite, "main", &new, &old).unwrap();
        assert!(back.sql.contains("DROP TABLE"), "{}", back.sql);
    }

    #[test]
    fn identical_states_have_no_script() {
        let t = [table("a", &[("id", "INTEGER")])];
        let d = diff(Dialect::Sqlite, "main", &t, &t).unwrap();
        assert_eq!(d.changes, Vec::new());
        assert!(d.sql.trim().is_empty(), "{}", d.sql);
    }
}
