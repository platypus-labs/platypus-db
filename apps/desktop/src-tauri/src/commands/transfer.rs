//! Import of data files (CSV, TSV, JSON, NDJSON, Parquet, XLSX) into a table. The file is
//! read through a DuckDB connection on it, the way it opens as a database.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::TryStreamExt;
use platypus_core::{Connection, ConnectionConfig, DriverKind, ExecOptions, ExecResult};
use platypus_driver_duckdb::files::is_data_file;
use platypus_export::import::{ImportError, ImportPlan, import, rows_text};
use platypus_sql::Dialect;
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
    let schema = file.schemas().await?.into_iter().next().map(|s| s.name);
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
    let d = Dialect::DuckDb;
    let sql = d.limited(
        &format!("SELECT * FROM {}", d.quote_ident(&table)),
        PREVIEW_ROWS,
    );
    let ExecResult::Rows(stream) = file.execute(&sql, &ExecOptions::default()).await? else {
        return Err(CommandError::new("import", "the file returned no rows"));
    };
    let schema = stream.schema();
    let batches: Vec<_> = stream.try_collect().await?;
    let columns = schema
        .fields()
        .iter()
        .map(|f| PreviewColumn {
            name: f.name().clone(),
            source_type: f
                .metadata()
                .get(platypus_core::META_DB_TYPE)
                .cloned()
                .unwrap_or_else(|| f.data_type().to_string()),
            target_type: dialect.column_type(f.data_type()),
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
    let session = state.session(session_id)?;
    let log = Arc::clone(&state.query_log);
    let job = Export::spawn(on_event, move |job| async move {
        let file = open_file(&path).await?;
        let target = Logged::wrap(
            Arc::from(super::connect(&session.config).await?),
            &log,
            &session.label,
        );
        let dialect = Dialect::from(session.config.driver);
        import(&*file, &*target, dialect, &plan, |rows| job.progress(rows))
            .await
            .map_err(|e| match e {
                ImportError::Database(e) => e.into(),
                e => CommandError::new("import", e.to_string()),
            })
    });
    let id = state.next_id();
    lock(&state.exports).insert(id, job);
    Ok(id)
}
