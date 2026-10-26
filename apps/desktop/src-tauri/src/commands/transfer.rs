//! Import of data files (CSV, TSV, JSON, NDJSON, Parquet, XLSX) into a table, and transfer of
//! a table to another database. A file is read through a DuckDB connection on it, the way it
//! opens as a database.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::TryStreamExt;
use platypus_core::{Connection, ConnectionConfig, DriverKind, ExecOptions, ExecResult};
use platypus_driver_duckdb::files::is_data_file;
use platypus_sql::Dialect;
use platypus_transfer::fake::{FakeColumn, fill, fill_collection};
use platypus_transfer::import::{ImportError, ImportPlan, import, rows_text, target_type};
use platypus_transfer::migrate::migrate;
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;

use super::{AppState, lock};
use crate::error::{CommandError, CommandResult};
use crate::export::{Export, ExportEvent};
use crate::querylog::Logged;

/// Rows the preview shows.
const PREVIEW_ROWS: u64 = 50;

#[derive(Debug, Serialize)]
pub struct PreviewColumn {
    name: String,
    /// As DuckDB read it, e.g. `BIGINT`.
    source_type: String,
    /// The column type that holds it in the target database.
    target_type: String,
}

#[derive(Debug, Serialize)]
pub struct ImportPreview {
    /// The tables of the file: one, or a workbook's sheets.
    tables: Vec<String>,
    table: String,
    columns: Vec<PreviewColumn>,
    rows: Vec<Vec<Option<String>>>,
}

async fn open_file(path: &Path) -> CommandResult<Box<dyn Connection>> {
    let path = path.to_string_lossy();
    if !is_data_file(&path) {
        return Err(CommandError::new(
            "import",
            "choose a CSV, TSV, JSON, NDJSON, Parquet or Excel file",
        ));
    }
    let mut config = ConnectionConfig::new(DriverKind::DuckDb);
    config.database = path.into_owned();
    Ok(super::connect(&config).await?)
}

/// The tables of the file at `path` and the first rows of `table` (by default the first),
/// with the column types they would take in the session's database.
#[tauri::command]
pub async fn preview_import(
    state: State<'_, AppState>,
    session_id: u64,
    path: PathBuf,
    table: Option<String>,
) -> CommandResult<ImportPreview> {
    let dialect = Dialect::from(state.session(session_id)?.config.driver);
    let file = open_file(&path).await?;
    let schema = file
        .schemas()
        .await?
        .into_iter()
        .find(|s| s.current)
        .map(|s| s.name);
    let tables: Vec<String> = match schema {
        Some(schema) => file
            .objects(&schema)
            .await?
            .into_iter()
            .map(|o| o.name)
            .collect(),
        None => Vec::new(),
    };
    let table = table
        .or_else(|| tables.first().cloned())
        .ok_or_else(|| CommandError::new("import", "the file has no data"))?;
    preview(&*file, (Dialect::DuckDb, None), tables, table, dialect).await
}

/// The first rows of `table` (in `schema`) read on `source`, with the column types they would
/// take in `target`.
async fn preview(
    source: &dyn Connection,
    (dialect, schema): (Dialect, Option<&str>),
    tables: Vec<String>,
    table: String,
    target: Dialect,
) -> CommandResult<ImportPreview> {
    let sql = dialect.limited(
        &format!("SELECT * FROM {}", dialect.qualified(schema, &table)),
        PREVIEW_ROWS,
    );
    let ExecResult::Rows(stream) = source.execute(&sql, &ExecOptions::default()).await? else {
        return Err(CommandError::new("import", "the source returned no rows"));
    };
    let fields = stream.schema();
    let batches: Vec<_> = stream.try_collect().await?;
    let columns = fields
        .fields()
        .iter()
        .map(|f| PreviewColumn {
            name: f.name().clone(),
            source_type: f
                .metadata()
                .get(platypus_core::META_DB_TYPE)
                .cloned()
                .unwrap_or_else(|| f.data_type().to_string()),
            target_type: target_type(f, target),
        })
        .collect();
    let mut rows = Vec::new();
    for batch in &batches {
        rows.extend(rows_text(batch).map_err(|e| CommandError::new("import", e.to_string()))?);
    }
    Ok(ImportPreview {
        tables,
        table,
        columns,
        rows,
    })
}

/// The first rows of `table` of the source session, with the column types they would take in
/// the target session's database. Read on a connection of its own, outside any transaction
/// of the session.
#[tauri::command]
pub async fn preview_transfer(
    state: State<'_, AppState>,
    source_session_id: u64,
    schema: Option<String>,
    table: String,
    target_session_id: u64,
) -> CommandResult<ImportPreview> {
    let config = state.session(source_session_id)?.config;
    let target = Dialect::from(state.session(target_session_id)?.config.driver);
    let source = super::connect(&config).await?;
    let dialect = Dialect::from(config.driver);
    preview(
        &*source,
        (dialect, schema.as_deref()),
        vec![table.clone()],
        table,
        target,
    )
    .await
}

/// Imports the file at `path` into the session's database on a connection of its own, as
/// `plan` says. Returns an id for `cancel_export`; progress and the outcome arrive on
/// `on_event`.
#[tauri::command]
pub fn import_file(
    state: State<'_, AppState>,
    session_id: u64,
    path: PathBuf,
    plan: ImportPlan,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    let source = async move { Ok((open_file(&path).await?, Dialect::DuckDb)) };
    start(&state, session_id, source, plan, on_event)
}

/// Copies a table of the source session's database into the target session's, streaming on
/// connections of their own, as `plan` says. Returns an id for `cancel_export`.
#[tauri::command]
pub fn transfer_table(
    state: State<'_, AppState>,
    source_session_id: u64,
    target_session_id: u64,
    plan: ImportPlan,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    let config = state.session(source_session_id)?.config;
    let source = async move {
        let dialect = Dialect::from(config.driver);
        Ok((super::connect(&config).await?, dialect))
    };
    start(&state, target_session_id, source, plan, on_event)
}

/// Runs the import of `plan` from the connection `source` opens into a new connection of
/// the target session.
fn start(
    state: &AppState,
    target_session_id: u64,
    source: impl Future<Output = CommandResult<(Box<dyn Connection>, Dialect)>> + Send + 'static,
    plan: ImportPlan,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    spawn_job(
        state,
        target_session_id,
        on_event,
        move |job, target, dialect| async move {
            let (source, source_dialect) = source.await?;
            import(
                (&*source, source_dialect),
                (&*target, dialect),
                &plan,
                |rows| {
                    job.progress(rows);
                    !job.stopping()
                },
            )
            .await
            .map_err(import_error)
        },
    )
}

/// Creates `tables` of the source session's `source_schema` in the target session's
/// `target_schema` and copies their rows, then their defaults, indexes, foreign keys and
/// CHECKs, and with `objects` the schema's views and routines; what is left out arrives as
/// warnings. Returns an id for `cancel_export`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn migrate_tables(
    state: State<'_, AppState>,
    source_session_id: u64,
    target_session_id: u64,
    source_schema: String,
    target_schema: String,
    tables: Vec<String>,
    objects: bool,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    let source = state.session(source_session_id)?;
    spawn_job(
        &state,
        target_session_id,
        on_event,
        move |job, to, dialect| async move {
            let from = super::connect(&source.config).await?;
            let done = migrate(
                (&*from, Dialect::from(source.config.driver), &source_schema),
                (&*to, dialect, &target_schema),
                &tables,
                objects,
                |rows| {
                    job.progress(rows);
                    !job.stopping()
                },
            )
            .await
            .map_err(import_error)?;
            for warning in done.skipped {
                job.warn(warning);
            }
            Ok(done.rows)
        },
    )
}

/// Adds `rows` fake rows to `table` of the session's database, on a connection of its own,
/// each column filled as `columns` says. Returns an id for `cancel_export`.
#[tauri::command]
pub fn fake_rows(
    state: State<'_, AppState>,
    session_id: u64,
    schema: Option<String>,
    table: String,
    rows: u64,
    columns: Vec<FakeColumn>,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    // MongoDB takes mongosh: documents go in with `insertMany`, `schema` is the database.
    let mongo = state.session(session_id)?.config.driver == DriverKind::Mongo;
    spawn_job(
        &state,
        session_id,
        on_event,
        move |job, target, dialect| async move {
            let progress = |n| {
                job.progress(n);
                !job.stopping()
            };
            let at = (schema.as_deref(), table.as_str());
            if mongo {
                fill_collection(&*target, at, columns, rows, progress).await
            } else {
                fill((&*target, dialect), at, columns, rows, progress).await
            }
            .map_err(import_error)
        },
    )
}

/// Runs `work` as a cancellable job on a new connection of the session, whose statements go
/// to the query log. Returns an id for `cancel_export`.
fn spawn_job<F>(
    state: &AppState,
    session_id: u64,
    on_event: Channel<ExportEvent>,
    work: impl FnOnce(Arc<Export>, Arc<dyn Connection>, Dialect) -> F + Send + 'static,
) -> CommandResult<u64>
where
    F: Future<Output = CommandResult<u64>> + Send + 'static,
{
    let session = state.session(session_id)?;
    let log = Arc::clone(&state.query_log);
    let job = Export::spawn(on_event, true, move |job| async move {
        let target = Logged::wrap(
            Arc::from(super::connect(&session.config).await?),
            &log,
            &session.label,
        );
        work(job, target, Dialect::from(session.config.driver)).await
    });
    let id = state.next_id();
    lock(&state.exports).insert(id, job);
    Ok(id)
}

fn import_error(e: ImportError) -> CommandError {
    match e {
        ImportError::Database(e) => e.into(),
        e => CommandError::new("import", e.to_string()),
    }
}
