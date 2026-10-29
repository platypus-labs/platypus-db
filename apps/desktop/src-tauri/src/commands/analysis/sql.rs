//! "Query with SQL": a result's rows as tables of an in-memory DuckDB, reached through a
//! temporary connection that is never saved. Every session opened on it (editor, navigator,
//! export) sees the same tables; the database lives until `close_result_db` and the last of
//! them close.

use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use platypus_core::{ConnectionConfig, DriverKind};
use platypus_driver_duckdb::MemoryDatabase;
use platypus_store::SavedConnection;
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;
use uuid::Uuid;

use super::gone;
use crate::commands::summary::blocking;
use crate::commands::{AppState, lock};
use crate::error::{CommandError, CommandResult};
use crate::jobs::Ending;

/// How often `fetch_whole` reports the rows loaded.
const PROGRESS: Duration = Duration::from_millis(100);

/// The in-memory database behind a temporary connection.
#[derive(Debug)]
pub struct ResultDb {
    saved: SavedConnection,
    /// Its name for `platypus_driver_duckdb::memory_database`.
    name: String,
    /// The tables loaded last: a reload drops those it no longer has.
    tables: Vec<String>,
    /// Keeps the database while the connection is open.
    _database: MemoryDatabase,
}

/// A table to make and the result whose rows it copies.
#[derive(Debug, Deserialize)]
pub struct ResultTable {
    job_id: u64,
    table: String,
}

#[derive(Debug, Serialize)]
pub struct OpenedResultDb {
    connection: SavedConnection,
    /// Set when a table lacks rows of its result.
    partial: Option<Partial>,
}

/// Why a table lacks rows of its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Partial {
    /// Reading stopped at the memory limit.
    MemoryLimit,
    /// Reading stopped for another reason: an error, a cancel or a script's row limit.
    Stopped,
}

/// The rows a table copies, taken from its result's job.
struct Source {
    table: String,
    schema: SchemaRef,
    batches: Vec<RecordBatch>,
    ending: Ending,
}

/// Whether, and why, one of `sources` lacks rows; the memory limit is told first.
fn partial(sources: &[Source]) -> Option<Partial> {
    if sources.iter().any(|s| s.ending == Ending::MemoryLimit) {
        Some(Partial::MemoryLimit)
    } else if sources.iter().any(|s| s.ending != Ending::Complete) {
        Some(Partial::Stopped)
    } else {
        None
    }
}

/// Loads every row of result `job_id`, up to the memory limit, reporting the rows loaded on
/// `on_progress`; `true` when they are all of the result's rows. It ends with the job: at the
/// end of the rows, the memory limit, `stop_fetch`, an error, or when the result is closed.
#[tauri::command]
pub async fn fetch_whole(
    state: State<'_, AppState>,
    job_id: u64,
    on_progress: Channel<usize>,
) -> CommandResult<bool> {
    let job = state.job(job_id).map_err(|_| gone())?;
    // A closed channel only means the dialog went away.
    let ending = job
        .fetch_whole(PROGRESS, |rows| {
            let _ = on_progress.send(rows);
        })
        .await;
    whole_fetched(ending)
}

/// What `fetch_whole` answers when the job ended with `ending`.
fn whole_fetched(ending: Ending) -> CommandResult<bool> {
    match ending {
        Ending::Closed => Err(gone()),
        Ending::Failed => Err(CommandError::new(
            "fetch_failed",
            "Reading the rows stopped with an error: the result shows it.",
        )),
        ending => Ok(ending == Ending::Complete),
    }
}

/// Stops loading result `job_id`, keeping the rows it has: Cancel of `fetch_whole`. A result
/// already closed has nothing to stop.
#[tauri::command]
pub fn stop_fetch(state: State<'_, AppState>, job_id: u64) {
    if let Ok(job) = state.job(job_id) {
        job.stop();
    }
}

/// Copies the rows of the results into a new in-memory DuckDB, as the tables named, and returns
/// the temporary connection to it, called `name`.
#[tauri::command]
pub async fn open_result_db(
    state: State<'_, AppState>,
    name: String,
    tables: Vec<ResultTable>,
) -> CommandResult<OpenedResultDb> {
    let sources = sources(&state, &tables)?;
    let partial = partial(&sources);
    let names: Vec<String> = sources.iter().map(|s| s.table.clone()).collect();
    let memory = format!("platypus-result-{}", Uuid::new_v4());
    let filled = memory.clone();
    let database = blocking(move || fill(&filled, &[], &sources)).await?;
    let saved = temporary(&name, &memory);
    let connection = saved.clone();
    lock(&state.result_dbs).insert(
        saved.id,
        ResultDb {
            saved,
            name: memory,
            tables: names,
            _database: database,
        },
    );
    Ok(OpenedResultDb {
        connection,
        partial,
    })
}

/// Copies the results again into the database of temporary connection `connection_id`;
/// whether, and why, one of them is partial.
#[tauri::command]
pub async fn reload_result_db(
    state: State<'_, AppState>,
    connection_id: Uuid,
    tables: Vec<ResultTable>,
) -> CommandResult<Option<Partial>> {
    let (memory, before) = lock(&state.result_dbs)
        .get(&connection_id)
        .map(|db| (db.name.clone(), db.tables.clone()))
        .ok_or_else(|| CommandError::not_found("result database", connection_id))?;
    let sources = sources(&state, &tables)?;
    let partial = partial(&sources);
    let names: Vec<String> = sources.iter().map(|s| s.table.clone()).collect();
    blocking(move || fill(&memory, &before, &sources).map(drop)).await?;
    if let Some(db) = lock(&state.result_dbs).get_mut(&connection_id) {
        db.tables = names;
    }
    Ok(partial)
}

/// Forgets the database of a temporary connection whose last tab closed; it goes once its
/// sessions are closed too.
#[tauri::command]
pub fn close_result_db(state: State<'_, AppState>, connection_id: Uuid) {
    lock(&state.result_dbs).remove(&connection_id);
}

impl AppState {
    /// How to open a session on temporary connection `id`, with its name; `None` for others.
    pub(in crate::commands) fn result_db_config(
        &self,
        id: Uuid,
    ) -> Option<(ConnectionConfig, String)> {
        lock(&self.result_dbs)
            .get(&id)
            .map(|db| (db.saved.config(None), db.saved.name.clone()))
    }
}

/// The DuckDB connection, never saved, to the in-memory database `memory`.
fn temporary(name: &str, memory: &str) -> SavedConnection {
    let mut saved = SavedConnection::new(name, DriverKind::DuckDb);
    saved.database = platypus_driver_duckdb::memory_path(memory);
    saved
}

/// The rows of each table's result as loaded now.
fn sources(state: &AppState, tables: &[ResultTable]) -> CommandResult<Vec<Source>> {
    if tables.is_empty() {
        return Err(CommandError::new(
            "no_rows",
            "The tab has no result with rows.",
        ));
    }
    tables
        .iter()
        .map(|t| {
            let job = state.job(t.job_id).map_err(|_| gone())?;
            // Another database, even a temporary one, is where a restricted result does not go.
            state.exportable(job.session_id)?;
            let (schema, batches) = job.snapshot().ok_or_else(gone)?;
            Ok(Source {
                table: t.table.clone(),
                schema,
                batches,
                ending: if job.is_done() {
                    job.ending()
                } else {
                    Ending::Running
                },
            })
        })
        .collect()
}

/// Loads `sources` into the in-memory database `memory`, after dropping the tables of
/// `before` they no longer have.
fn fill(memory: &str, before: &[String], sources: &[Source]) -> CommandResult<MemoryDatabase> {
    let (database, conn) = platypus_driver_duckdb::memory_database(memory)?;
    for table in before
        .iter()
        .filter(|t| !sources.iter().any(|s| &s.table == *t))
    {
        platypus_local::duck::drop_table(&conn, table)?;
    }
    for s in sources {
        platypus_local::duck::load(&conn, &s.table, &s.schema, &s.batches)?;
    }
    Ok(database)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{Int32Array, RecordBatch};
    use arrow_schema::{DataType, Field, Schema};

    use super::*;
    use crate::commands::analysis::error_kind;

    fn source(table: &str, values: &[i32]) -> Source {
        let schema = Arc::new(Schema::new(vec![Field::new("n", DataType::Int32, false)]));
        let column = Arc::new(Int32Array::from(values.to_vec()));
        let batch = RecordBatch::try_new(Arc::clone(&schema), vec![column]).unwrap();
        Source {
            table: table.to_owned(),
            schema,
            batches: vec![batch],
            ending: Ending::Complete,
        }
    }

    fn one(memory: &str, sql: &str) -> i64 {
        let (_database, conn) = platypus_driver_duckdb::memory_database(memory).unwrap();
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_closed_result_cannot_be_copied() {
        platypus_driver_duckdb::testing::load();
        let state = AppState::for_tests();
        let missing = [ResultTable {
            job_id: 7,
            table: "result".to_owned(),
        }];
        let Err(e) = sources(&state, &missing) else {
            panic!("there is no job 7")
        };
        assert_eq!(error_kind(&e), "result_gone");
        let Err(e) = sources(&state, &[]) else {
            panic!("no tables")
        };
        assert_eq!(error_kind(&e), "no_rows");
    }

    #[test]
    fn a_partial_table_tells_why() {
        platypus_driver_duckdb::testing::load();
        let mut sources = vec![source("a", &[1]), source("b", &[2])];
        assert_eq!(partial(&sources), None);
        sources[0].ending = Ending::RowLimit;
        assert_eq!(partial(&sources), Some(Partial::Stopped));
        sources[1].ending = Ending::MemoryLimit;
        assert_eq!(partial(&sources), Some(Partial::MemoryLimit));
    }

    #[test]
    fn a_whole_fetch_ends_with_its_job() {
        platypus_driver_duckdb::testing::load();
        assert!(whole_fetched(Ending::Complete).unwrap());
        assert!(!whole_fetched(Ending::MemoryLimit).unwrap());
        assert!(!whole_fetched(Ending::Stopped).unwrap());
        let Err(e) = whole_fetched(Ending::Closed) else {
            panic!("a closed result is gone")
        };
        assert_eq!(error_kind(&e), "result_gone");
        let Err(e) = whole_fetched(Ending::Failed) else {
            panic!("a failed fetch is an error")
        };
        assert_eq!(error_kind(&e), "fetch_failed");
    }

    #[test]
    fn fills_and_reloads_the_tables() {
        platypus_driver_duckdb::testing::load();
        let memory = format!("tests-{}", Uuid::new_v4());
        let all = [
            source("result", &[1, 2, 3]),
            source("r1", &[1, 2, 3]),
            source("r2", &[4]),
        ];
        let _database = fill(&memory, &[], &all).unwrap();
        assert_eq!(one(&memory, "SELECT count(*) FROM result"), 3);
        assert_eq!(one(&memory, "SELECT sum(n)::BIGINT FROM r2"), 4);
        let before = ["result", "r1", "r2"].map(String::from);
        fill(&memory, &before, &[source("result", &[5])]).unwrap();
        assert_eq!(one(&memory, "SELECT count(*) FROM result"), 1);
        let gone = "SELECT count(*) FROM duckdb_tables() WHERE table_name IN ('r1', 'r2')";
        assert_eq!(one(&memory, gone), 0);
    }

    #[test]
    fn a_temporary_connection_opens_its_database() {
        platypus_driver_duckdb::testing::load();
        let state = AppState::for_tests();
        let memory = format!("tests-{}", Uuid::new_v4());
        let database = fill(&memory, &[], &[source("result", &[1])]).unwrap();
        let saved = temporary("Results of x", &memory);
        let id = saved.id;
        lock(&state.result_dbs).insert(
            id,
            ResultDb {
                saved,
                name: memory.clone(),
                tables: vec!["result".to_owned()],
                _database: database,
            },
        );
        let (config, label) = state.result_db_config(id).unwrap();
        assert_eq!(
            config.database,
            platypus_driver_duckdb::memory_path(&memory)
        );
        assert_eq!(label, "Results of x");
        assert!(state.result_db_config(Uuid::new_v4()).is_none());
    }
}
