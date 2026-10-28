//! The sample database of the welcome screen: `samples/wildlife.sql`, a SQLite platypus
//! field survey, built into the app's data folder at the first click, with a connection
//! and a few saved scripts on it.

use std::path::{Path, PathBuf};

use platypus_core::DriverKind;
use platypus_store::{PasswordUpdate, SavedConnection};
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::projects::Workspace;
use super::{AppState, scripts};
use crate::error::{CommandError, CommandResult};

const SQL: &str = include_str!("../../../../../samples/wildlife.sql");
const NAME: &str = "Platypus survey (sample)";
/// The Scripts panel's folder of the sample's scripts.
const FOLDER: &str = "Sample";
const SCRIPTS: &[(&str, &str)] = &[
    (
        "Sightings per river.sql",
        include_str!("../../../../../samples/scripts/Sightings per river.sql"),
    ),
    (
        "Most seen platypuses.sql",
        include_str!("../../../../../samples/scripts/Most seen platypuses.sql"),
    ),
    (
        "Weight by sex.sql",
        include_str!("../../../../../samples/scripts/Weight by sex.sql"),
    ),
    (
        "Surveys by researcher.sql",
        include_str!("../../../../../samples/scripts/Surveys by researcher.sql"),
    ),
];

#[derive(Serialize)]
pub struct SampleDatabase {
    workspace: Workspace,
    connection: Uuid,
    /// The first script, absolute: the app opens it.
    script: PathBuf,
}

fn io(path: &Path, e: impl std::fmt::Display) -> CommandError {
    CommandError::new("io", format!("{}: {e}", path.display()))
}

/// Builds the database at `path` unless it is there: through a temporary file, so a failed
/// build leaves nothing half made.
fn build(path: &Path) -> CommandResult<()> {
    if path.exists() {
        return Ok(());
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    let partial = path.with_extension("db.part");
    let _ = std::fs::remove_file(&partial);
    let db = rusqlite::Connection::open(&partial).map_err(|e| io(&partial, e))?;
    db.execute_batch(SQL).map_err(|e| io(&partial, e))?;
    db.close().map_err(|(_, e)| io(&partial, e))?;
    std::fs::rename(&partial, path).map_err(|e| io(path, e))
}

/// Builds the sample database if needed, saves its connection (in `project` too) unless
/// one opens it already, and adds its scripts to the project's.
#[tauri::command]
pub fn open_sample_database(
    state: State<'_, AppState>,
    project: Option<Uuid>,
) -> CommandResult<SampleDatabase> {
    // Next to the scripts folder, in the app's data folder.
    let scripts_dir = state
        .scripts
        .as_ref()
        .map_err(|e| CommandError::new("io", e.clone()))?;
    let path = scripts_dir.with_file_name("samples").join("wildlife.db");
    build(&path)?;
    let database = path.to_string_lossy().into_owned();
    let (connection, workspace) = state.with_store(|s| {
        let found = s
            .list()
            .iter()
            .find(|c| c.driver == DriverKind::Sqlite && c.database == database)
            .map(|c| c.id);
        let id = if let Some(id) = found {
            id
        } else {
            let mut connection = SavedConnection::new(NAME, DriverKind::Sqlite);
            connection.database.clone_from(&database);
            // No password: the keychain is never asked.
            connection.save_password = false;
            let id = connection.id;
            s.save(connection, PasswordUpdate::Keep)?;
            id
        };
        if let Some(project) = project {
            s.add_to_project(project, &[id])?;
        }
        Ok((id, Workspace::of(s)))
    })?;
    let root = scripts::root(&state, project)?;
    let paths = scripts::add_linked(&root, FOLDER, SCRIPTS, connection)?;
    Ok(SampleDatabase {
        workspace,
        connection,
        script: root.join(&paths[0]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_database_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("samples").join("wildlife.db");
        build(&path).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        let count = |table: &str| -> i64 {
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(count("sightings"), 15_000);
        assert_eq!(count("platypuses"), 300);
        // A second open keeps the user's database as it is.
        db.execute("CREATE TABLE mine (x)", []).unwrap();
        drop(db);
        build(&path).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        db.query_row("SELECT count(*) FROM mine", [], |r| r.get::<_, i64>(0))
            .unwrap();
        assert!(!path.with_extension("db.part").exists());
    }

    #[test]
    fn every_script_runs_on_the_sample() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(SQL).unwrap();
        for (name, sql) in SCRIPTS {
            let mut statement = db.prepare(sql).unwrap_or_else(|e| panic!("{name}: {e}"));
            let rows = statement.query_map([], |_| Ok(())).unwrap().count();
            assert!(rows > 0, "{name}: no rows");
        }
    }

    #[test]
    fn adds_the_scripts_once_and_keeps_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let connection = Uuid::new_v4();
        let paths = scripts::add_linked(root, FOLDER, SCRIPTS, connection).unwrap();
        assert_eq!(paths[0], "Sample/Sightings per river.sql");
        std::fs::write(root.join(&paths[0]), "-- mine").unwrap();
        scripts::add_linked(root, FOLDER, SCRIPTS, connection).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join(&paths[0])).unwrap(),
            "-- mine"
        );
        let links = std::fs::read_to_string(root.join(".links.json")).unwrap();
        assert_eq!(
            links.matches(&connection.to_string()).count(),
            SCRIPTS.len()
        );
    }
}
