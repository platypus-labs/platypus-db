//! Snapshots before an `UPDATE` or `DELETE` run from the editor: the rows it is about to
//! change, read on the session first and kept as the statements that put them back, one JSON
//! file each in `snapshots` under the app's data folder. The newest [`KEPT`] stay.

use std::fs;
use std::path::{Path, PathBuf};

use platypus_sql::snapshot::touched_rows;
use platypus_transfer::snapshot::{Taken, take};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use super::AppState;
use crate::error::{CommandError, CommandResult};

/// How many rows a snapshot reads at most: a change of more takes none.
const ROW_LIMIT: usize = 10_000;
/// How many snapshots are kept.
const KEPT: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub id: String,
    pub connection_id: Uuid,
    pub connection: String,
    pub database: Option<String>,
    /// `UPDATE` or `DELETE`.
    pub verb: String,
    pub table: String,
    /// The statement the rows were read before.
    pub statement: String,
    pub rows: u64,
    /// Milliseconds since the Unix epoch.
    pub taken_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotFile {
    #[serde(flatten)]
    meta: SnapshotMeta,
    restore: Vec<String>,
}

/// What a snapshot before a statement came to: kept, or why not.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SnapshotOutcome {
    Taken {
        snapshot: SnapshotMeta,
    },
    /// Not one table's rows a `SELECT` tells (a join, a `LIMIT`), no change at all, or rows
    /// that may not be exported.
    Unsupported,
    TooMany {
        limit: usize,
    },
    /// An `UPDATE` of a table without a primary key.
    NoKey,
}

fn io(path: &Path, e: &std::io::Error) -> CommandError {
    CommandError::new("io", format!("{}: {e}", path.display()))
}

/// The snapshots' folder, created on first use.
fn folder(state: &AppState) -> CommandResult<PathBuf> {
    let scripts = state
        .scripts
        .as_ref()
        .map_err(|e| CommandError::new("io", e.clone()))?;
    let dir = scripts
        .parent()
        .map_or_else(|| PathBuf::from("snapshots"), |p| p.join("snapshots"));
    fs::create_dir_all(&dir).map_err(|e| io(&dir, &e))?;
    Ok(dir)
}

/// A snapshot's file; ids are the app's own, checked so that none reaches outside.
fn file(state: &AppState, id: &str) -> CommandResult<PathBuf> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(CommandError::not_found("snapshot", id));
    }
    Ok(folder(state)?.join(format!("{id}.json")))
}

/// Reads the rows `sql` is about to change on the session, in its transaction, and keeps them
/// as the statements that restore them.
#[tauri::command]
pub async fn take_snapshot(
    state: State<'_, AppState>,
    session_id: u64,
    database: Option<String>,
    sql: String,
) -> CommandResult<SnapshotOutcome> {
    let dialect = state.dialect(session_id)?;
    // Rows that may not leave the connection are not written to a file either.
    if touched_rows(&sql, dialect).is_none() || state.exportable(session_id).is_err() {
        return Ok(SnapshotOutcome::Unsupported);
    }
    // The session as it is: a dropped connection is reopened, and reported, by the statement
    // that follows, not here.
    let session = state.session(session_id)?;
    if session.conn.is_closed() {
        return Ok(SnapshotOutcome::Unsupported);
    }
    let snapshot = match take(&*session.conn, dialect, &sql, ROW_LIMIT).await {
        Ok(Taken::Snapshot(snapshot)) => snapshot,
        Ok(Taken::Unsupported) => return Ok(SnapshotOutcome::Unsupported),
        Ok(Taken::TooMany) => return Ok(SnapshotOutcome::TooMany { limit: ROW_LIMIT }),
        Ok(Taken::NoKey) => return Ok(SnapshotOutcome::NoKey),
        Err(e) => return Err(CommandError::new("snapshot", e.to_string())),
    };
    let taken_at = chrono::Utc::now().timestamp_millis();
    let meta = SnapshotMeta {
        id: format!("{taken_at}-{}", &Uuid::new_v4().simple().to_string()[..8]),
        connection_id: session.connection_id,
        connection: session.label.clone(),
        database,
        verb: snapshot.verb.to_owned(),
        table: snapshot.table,
        statement: sql,
        rows: snapshot.rows,
        taken_at,
    };
    let dir = folder(&state)?;
    let path = dir.join(format!("{}.json", meta.id));
    let text = serde_json::to_string(&SnapshotFile {
        meta: meta.clone(),
        restore: snapshot.restore,
    })
    .map_err(|e| CommandError::new("snapshot", e.to_string()))?;
    fs::write(&path, text).map_err(|e| io(&path, &e))?;
    prune(&dir);
    Ok(SnapshotOutcome::Taken { snapshot: meta })
}

/// Removes the oldest snapshots past [`KEPT`]; their names start with the time they were
/// taken.
fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    if names.len() <= KEPT {
        return;
    }
    names.sort();
    for old in &names[..names.len() - KEPT] {
        let _ = fs::remove_file(old);
    }
}

/// The snapshots kept, newest first.
#[tauri::command]
pub fn list_snapshots(state: State<'_, AppState>) -> CommandResult<Vec<SnapshotMeta>> {
    let dir = folder(&state)?;
    let mut all: Vec<SnapshotMeta> = fs::read_dir(&dir)
        .map_err(|e| io(&dir, &e))?
        .filter_map(Result::ok)
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter_map(|text| serde_json::from_str::<SnapshotFile>(&text).ok())
        .map(|f| f.meta)
        .collect();
    all.sort_by_key(|m| std::cmp::Reverse(m.taken_at));
    Ok(all)
}

/// The statements that put a snapshot's rows back.
#[tauri::command]
pub fn snapshot_restore(state: State<'_, AppState>, id: String) -> CommandResult<Vec<String>> {
    let path = file(&state, &id)?;
    let text = fs::read_to_string(&path).map_err(|_| CommandError::not_found("snapshot", &id))?;
    let file: SnapshotFile =
        serde_json::from_str(&text).map_err(|e| CommandError::new("snapshot", e.to_string()))?;
    Ok(file.restore)
}

#[tauri::command]
pub fn delete_snapshot(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let path = file(&state, &id)?;
    fs::remove_file(&path).map_err(|e| io(&path, &e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..KEPT + 3 {
            fs::write(dir.path().join(format!("{:013}-a.json", 1000 + n)), "{}").unwrap();
        }
        prune(dir.path());
        let mut left: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left.len(), KEPT);
        assert_eq!(left[0], format!("{:013}-a.json", 1003));
    }

    #[test]
    fn ids_stay_in_the_folder() {
        let state = AppState::for_tests();
        for id in ["../x", "a/b", "", "a.json"] {
            assert!(file(&state, id).is_err(), "{id}");
        }
    }
}
