//! Import of data files (CSV, TSV, JSON, NDJSON, Parquet, XLSX) into a table, and transfer of
//! a table to another database. A file is read through a DuckDB connection on it, the way it
//! opens as a database.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::TryStreamExt;
use platypus_core::{
    Connection, ConnectionConfig, DriverKind, ExecOptions, ExecResult, ObjectKind,
};
use platypus_driver_duckdb::files::{BAD_LINES, ReadOptions, is_data_file};
use platypus_sql::Dialect;
use platypus_transfer::dump::{DumpFile, dump};
use platypus_transfer::fake::{FakeColumn, fill, fill_collection};
use platypus_transfer::import::{
    ImportError, ImportPlan, import, import_query, rows_text, select_all, target_type,
};
use platypus_transfer::migrate::{ColumnTraits, column_traits, migrate};
use platypus_transfer::script::{ScriptError, run_file};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;

use super::{AppState, lock};
use crate::error::{CommandError, CommandResult};
use crate::export::{Export, ExportEvent, PROGRESS_INTERVAL};
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
    /// What a new table keeps of a source table's column; none for a file.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    traits: Option<ColumnTraits>,
}

/// How a delimited file is read, as chosen in the import dialog.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileOptions {
    /// Its encoding's label; none tells it from the bytes.
    pub encoding: Option<String>,
    /// Lines with another number of values are left out.
    pub skip_bad_lines: bool,
}

#[derive(Debug, Serialize)]
pub struct ImportPreview {
    /// The tables of the file: one, or a workbook's sheets.
    tables: Vec<String>,
    table: String,
    columns: Vec<PreviewColumn>,
    rows: Vec<Vec<Option<String>>>,
}

async fn open_file(path: &Path, options: &FileOptions) -> CommandResult<Box<dyn Connection>> {
    let path = path.to_string_lossy();
    if !is_data_file(&path) {
        return Err(CommandError::new(
            "import",
            "choose a CSV, TSV, JSON, NDJSON, Parquet or Excel file",
        ));
    }
    let mut config = ConnectionConfig::new(DriverKind::DuckDb);
    config.database = path.into_owned();
    if let Some(encoding) = &options.encoding {
        config
            .options
            .insert(ReadOptions::ENCODING.to_owned(), encoding.clone());
    }
    if options.skip_bad_lines {
        config
            .options
            .insert(ReadOptions::SKIP_BAD_LINES.to_owned(), "true".to_owned());
    }
    super::connect(&config).await.map_err(|e| {
        // Lines that do not fit can be left out: the dialog offers it.
        let e = CommandError::from(e);
        if e.message().contains(BAD_LINES) {
            e.with_kind("bad_lines")
        } else {
            e
        }
    })
}

/// The tables of the file at `path` and the first rows of `table` (by default the first),
/// with the column types they would take in the session's database.
#[tauri::command]
pub async fn preview_import(
    state: State<'_, AppState>,
    session_id: u64,
    path: PathBuf,
    table: Option<String>,
    options: Option<FileOptions>,
) -> CommandResult<ImportPreview> {
    let dialect = state.dialect(session_id)?;
    let file = open_file(&path, &options.unwrap_or_default()).await?;
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
    preview(&*file, (DriverKind::DuckDb, None), tables, table, dialect).await
}

/// The query of every row of a table of `driver` (`limit` at most): SQL, `find`, `BROWSE`, or
/// a search of the index.
pub(super) fn read_all(driver: DriverKind, at: (Option<&str>, &str), limit: Option<u64>) -> String {
    if driver == DriverKind::Elasticsearch {
        return platypus_driver_search::command::browse_limited(at.1, limit);
    }
    select_all(driver, at, limit)
}

/// The first rows of `table` (in `schema`) read on `source`, with the column types they would
/// take in `target`.
async fn preview(
    source: &dyn Connection,
    (driver, schema): (DriverKind, Option<&str>),
    tables: Vec<String>,
    table: String,
    target: Dialect,
) -> CommandResult<ImportPreview> {
    let sql = read_all(driver, (schema, &table), Some(PREVIEW_ROWS));
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
            traits: None,
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
    let target = state.dialect(target_session_id)?;
    let source = super::connect(&config).await?;
    let mut shown = preview(
        &*source,
        (config.driver, schema.as_deref()),
        vec![table.clone()],
        table.clone(),
        target,
    )
    .await?;
    if !matches!(
        config.driver,
        DriverKind::Mongo | DriverKind::Redis | DriverKind::Elasticsearch
    ) {
        let at = (schema.as_deref(), table.as_str());
        let source = (&*source, Dialect::from(config.driver));
        // Without them the copy still works, as a plain table.
        if let Ok(traits) = column_traits(source, at, target).await {
            for column in &mut shown.columns {
                column.traits = traits.iter().find(|t| t.name == column.name).cloned();
            }
        }
    }
    Ok(shown)
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
    options: Option<FileOptions>,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    state.editable(session_id)?;
    let session = state.session(session_id)?;
    Ok(start_file(
        &state,
        (session.config, session.label),
        (path, options.unwrap_or_default(), plan),
        on_event,
    ))
}

/// Starts the import of the file at `path` into the database of `target` (its settings and
/// its name in the query log); returns the id `cancel_export` takes. The caller has checked
/// what the target allows.
pub(crate) fn start_file(
    state: &AppState,
    target: (ConnectionConfig, String),
    (path, options, plan): (PathBuf, FileOptions, ImportPlan),
    on_event: Channel<ExportEvent>,
) -> u64 {
    let source = async move {
        let file = open_file(&path, &options).await?;
        Ok((file, DriverKind::DuckDb))
    };
    start(state, target, source, plan, on_event)
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
    state.exportable(source_session_id)?;
    state.editable(target_session_id)?;
    let source = state.session(source_session_id)?.config;
    let target = state.session(target_session_id)?;
    Ok(start_copy(
        &state,
        source,
        (target.config, target.label),
        plan,
        on_event,
    ))
}

/// Starts the copy of a table of the database `source` opens into that of `target`; returns
/// the id `cancel_export` takes. The caller has checked what the two allow.
pub(crate) fn start_copy(
    state: &AppState,
    source: ConnectionConfig,
    target: (ConnectionConfig, String),
    plan: ImportPlan,
    on_event: Channel<ExportEvent>,
) -> u64 {
    let driver = source.driver;
    let source = async move { Ok((super::connect(&source).await?, driver)) };
    start(state, target, source, plan, on_event)
}

/// Runs the import of `plan` from the connection `source` opens into a new connection of
/// the target.
fn start(
    state: &AppState,
    target: (ConnectionConfig, String),
    source: impl Future<Output = CommandResult<(Box<dyn Connection>, DriverKind)>> + Send + 'static,
    plan: ImportPlan,
    on_event: Channel<ExportEvent>,
) -> u64 {
    spawn_on(
        state,
        target,
        on_event,
        move |job, target, dialect| async move {
            let (source, driver) = source.await?;
            let progress = |rows| {
                job.progress(rows);
                !job.stopping()
            };
            let target = (&*target, dialect);
            // MongoDB reads its collection with `find`, Redis a key type with `BROWSE`,
            // Elasticsearch an index with a search.
            if matches!(
                driver,
                DriverKind::Mongo | DriverKind::Redis | DriverKind::Elasticsearch
            ) {
                let at = (plan.source_schema.as_deref(), plan.source_table.as_str());
                let query = read_all(driver, at, None);
                import_query(&*source, &query, target, &plan, progress).await
            } else {
                import((&*source, Dialect::from(driver)), target, &plan, progress).await
            }
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
    state.exportable(source_session_id)?;
    state.editable(target_session_id)?;
    let source = state.session(source_session_id)?.config;
    let target = state.session(target_session_id)?;
    Ok(start_migrate(
        &state,
        source,
        (target.config, target.label),
        (source_schema, target_schema, Some(tables), objects),
        on_event,
    ))
}

/// Starts the migration of `tables` of `source_schema` in the database `source` opens to
/// `target_schema` in that of `target` (`None`: every table of the schema, read when it
/// runs, so those added later come too); returns the id `cancel_export` takes. The caller has
/// checked what the two allow.
pub(crate) fn start_migrate(
    state: &AppState,
    source: ConnectionConfig,
    target: (ConnectionConfig, String),
    (source_schema, target_schema, tables, objects): (String, String, Option<Vec<String>>, bool),
    on_event: Channel<ExportEvent>,
) -> u64 {
    spawn_on(
        state,
        target,
        on_event,
        move |job, to, dialect| async move {
            let from = super::connect(&source).await?;
            let tables = match tables {
                Some(tables) => tables,
                None => from
                    .objects(&source_schema)
                    .await?
                    .into_iter()
                    .filter(|o| o.kind == ObjectKind::Table)
                    .map(|o| o.name)
                    .collect(),
            };
            let done = migrate(
                (&*from, Dialect::from(source.driver), &source_schema),
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
    state.editable(session_id)?;
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
    // The jobs here write into the session's database, but a dump.
    state.writable(session_id)?;
    spawn_reading_job(state, session_id, on_event, work)
}

/// [`spawn_job`] for a job that only reads, which a read-only connection runs too.
fn spawn_reading_job<F>(
    state: &AppState,
    session_id: u64,
    on_event: Channel<ExportEvent>,
    work: impl FnOnce(Arc<Export>, Arc<dyn Connection>, Dialect) -> F + Send + 'static,
) -> CommandResult<u64>
where
    F: Future<Output = CommandResult<u64>> + Send + 'static,
{
    let session = state.session(session_id)?;
    Ok(spawn_on(
        state,
        (session.config, session.label),
        on_event,
        work,
    ))
}

/// [`spawn_reading_job`] on a new connection of `config`, named `label` in the query log.
fn spawn_on<F>(
    state: &AppState,
    (config, label): (ConnectionConfig, String),
    on_event: Channel<ExportEvent>,
    work: impl FnOnce(Arc<Export>, Arc<dyn Connection>, Dialect) -> F + Send + 'static,
) -> u64
where
    F: Future<Output = CommandResult<u64>> + Send + 'static,
{
    let log = Arc::clone(&state.query_log);
    let job = Export::spawn(on_event, true, move |job| async move {
        let target = Logged::wrap(Arc::from(super::connect(&config).await?), &log, &label);
        let dialect = Dialect::of_server(config.driver, &target.server_info().product);
        work(job, target, dialect).await
    });
    let id = state.next_id();
    lock(&state.exports).insert(id, job);
    id
}

/// Runs the SQL file at `path` (`.gz` and `.zst` too) in the session's database, on a
/// connection of its own; with `transaction`, all in one, which a failure rolls back.
/// Progress counts statements and the bytes of the file read; cancelling stops the running
/// statement too. Returns an id for `cancel_export`.
#[tauri::command]
pub fn run_sql_file(
    state: State<'_, AppState>,
    session_id: u64,
    path: PathBuf,
    transaction: bool,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    spawn_job(
        &state,
        session_id,
        on_event,
        move |job, conn, dialect| async move {
            // Cancel stops the statement that runs, not only those after it.
            let watch = {
                let (job, cancel) = (Arc::clone(&job), conn.cancel_handle());
                tauri::async_runtime::spawn(async move {
                    while !job.stopping() {
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                    let _ = cancel.cancel().await;
                })
            };
            // A file of small statements reports a few times a second, not after each.
            let mut reported: Option<std::time::Instant> = None;
            let ran = run_file(&*conn, dialect, &path, transaction, |p| {
                if reported.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL) {
                    job.progress_bytes(p.statements, p.read, p.total);
                    reported = Some(std::time::Instant::now());
                }
                !job.stopping()
            })
            .await;
            watch.abort();
            ran.map_err(|e| match e {
                ScriptError::Database(e) => e.into(),
                ScriptError::Statement { error, .. }
                    if matches!(*error, platypus_core::Error::Cancelled) =>
                {
                    platypus_core::Error::Cancelled.into()
                }
                e => CommandError::new("script", e.to_string()),
            })
        },
    )
}

/// Writes `schema` of the session's database to an SQL file at `path` (gzip or zstd by its
/// extension): its DDL, and with `data` its rows; with `tables`, those objects alone. What
/// the database cannot describe arrives as warnings. Returns an id for `cancel_export`.
#[tauri::command]
pub fn dump_sql(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    path: PathBuf,
    data: bool,
    tables: Option<Vec<String>>,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    state.exportable(session_id)?;
    spawn_reading_job(
        &state,
        session_id,
        on_event,
        move |job, conn, dialect| async move {
            let mut out = DumpFile::create(&path)
                .map_err(|e| CommandError::new("export", format!("{}: {e}", path.display())))?;
            let dumped = dump(
                (&*conn, dialect, &schema),
                &mut out,
                (data, tables.as_deref()),
                |rows| {
                    job.progress(rows);
                    !job.stopping()
                },
            )
            .await;
            // A failed or stopped dump leaves the file it would have replaced as it was.
            let dumped = match dumped {
                Ok(dumped) => dumped,
                Err(e) => {
                    out.discard();
                    return Err(import_error(e));
                }
            };
            out.finish()
                .map_err(|e| CommandError::new("export", format!("{}: {e}", path.display())))?;
            for warning in dumped.skipped {
                job.warn(warning);
            }
            Ok(dumped.rows)
        },
    )
}

fn import_error(e: ImportError) -> CommandError {
    match e {
        ImportError::Database(e) => e.into(),
        e => CommandError::new("import", e.to_string()),
    }
}
