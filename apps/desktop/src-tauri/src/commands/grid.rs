//! Grid features on loaded results: copying a selection in other formats, searching the
//! loaded rows, the value panel and quick filters. They read the job's buffered rows, so they
//! never touch the session.

use std::fmt::Write as _;

use arrow_array::Array;
use arrow_array::cast::AsArray;
use arrow_cast::display::ArrayFormatter;
use platypus_sql::Dialect;
use platypus_sql::dml::{Column, ValueKind};
use serde::Serialize;
use tauri::State;
use tauri::ipc::Response;

use super::AppState;
use super::data::rows_text;
use crate::copy::{self, CopyFormat, Selection, Target};
use crate::display;
use crate::error::{CommandError, CommandResult};
use crate::jobs::Job;

/// Matches reported at most by a search.
const SEARCH_LIMIT: usize = 10_000;

fn dialect(state: &AppState, job: &Job) -> CommandResult<Dialect> {
    Ok(Dialect::from(state.session(job.session_id)?.config.driver))
}

fn schema(job: &Job) -> CommandResult<arrow_schema::SchemaRef> {
    job.window(0, 0)
        .map(|(schema, _)| schema)
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))
}

fn check_column(schema: &arrow_schema::Schema, column: usize) -> CommandResult<()> {
    if column < schema.fields().len() {
        Ok(())
    } else {
        Err(CommandError::not_found("column", column + 1))
    }
}

/// Loaded rows `offset..offset + limit`, columns `columns`, in `format`. `table` names the
/// table in `INSERT` statements.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn copy_rows(
    state: State<'_, AppState>,
    job_id: u64,
    offset: usize,
    limit: usize,
    columns: Vec<usize>,
    format: CopyFormat,
    header: bool,
    table: Option<String>,
) -> CommandResult<String> {
    let job = state.job(job_id)?;
    let schema = schema(&job)?;
    for &c in &columns {
        check_column(&schema, c)?;
    }
    let rows = rows_text(&job, offset, limit)?
        .into_iter()
        .map(|mut row| columns.iter().map(|&c| row[c].take()).collect())
        .collect();
    let selection = Selection {
        names: columns
            .iter()
            .map(|&c| schema.field(c).name().as_str())
            .collect(),
        kinds: columns
            .iter()
            .map(|&c| ValueKind::of(schema.field(c).data_type()))
            .collect(),
        rows,
    };
    let target = Target {
        dialect: dialect(&state, &job)?,
        table: table.as_deref().unwrap_or("table_name"),
        header,
    };
    copy::render(format, &selection, &target)
        .map_err(|e| CommandError::new("invalid_value", e.to_string()))
}

/// The values of `column` in loaded rows `offset..offset + limit` as SQL literals (`NULL`
/// included), for filters built from the grid.
#[tauri::command]
pub fn sql_literals(
    state: State<'_, AppState>,
    job_id: u64,
    offset: usize,
    limit: usize,
    column: usize,
) -> CommandResult<Vec<String>> {
    let job = state.job(job_id)?;
    let schema = schema(&job)?;
    check_column(&schema, column)?;
    let field = schema.field(column);
    let c = Column {
        name: field.name(),
        kind: ValueKind::of(field.data_type()),
        default: None,
    };
    let dialect = dialect(&state, &job)?;
    rows_text(&job, offset, limit)?
        .iter()
        .map(|row| {
            c.literal(dialect, row[column].as_deref())
                .map_err(|e| CommandError::new("invalid_value", e.to_string()))
        })
        .collect()
}

/// The raw bytes of a cell: binary values as stored, anything else as UTF-8 text.
/// `None` for NULL.
fn cell_bytes_of(job: &Job, row: usize, column: usize) -> CommandResult<Option<Vec<u8>>> {
    let (schema, batches) = job
        .window(row, 1)
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    check_column(&schema, column)?;
    let batch = batches
        .first()
        .ok_or_else(|| CommandError::not_found("row", row + 1))?;
    let array = batch.column(column);
    if array.is_null(0) {
        return Ok(None);
    }
    let bytes = if let Some(a) = array.as_binary_opt::<i32>() {
        a.value(0).to_vec()
    } else if let Some(a) = array.as_binary_opt::<i64>() {
        a.value(0).to_vec()
    } else if let Some(a) = array.as_binary_view_opt() {
        a.value(0).to_vec()
    } else if let Some(a) = array.as_fixed_size_binary_opt() {
        a.value(0).to_vec()
    } else {
        let mut rows = Vec::with_capacity(1);
        display::rows_text(batch, &mut rows)?;
        rows.pop()
            .and_then(|mut r| r[column].take())
            .unwrap_or_default()
            .into_bytes()
    };
    Ok(Some(bytes))
}

/// A cell's raw bytes for the value panel (images, hex); an empty answer is NULL or empty.
#[tauri::command]
pub fn cell_bytes(
    state: State<'_, AppState>,
    job_id: u64,
    row: usize,
    column: usize,
) -> CommandResult<Response> {
    let bytes = cell_bytes_of(&*state.job(job_id)?, row, column)?;
    Ok(Response::new(bytes.unwrap_or_default()))
}

/// Writes a cell's value to `path`: binary values as stored, others as UTF-8 text.
#[tauri::command]
pub async fn save_cell(
    state: State<'_, AppState>,
    job_id: u64,
    row: usize,
    column: usize,
    path: String,
) -> CommandResult<()> {
    let bytes = cell_bytes_of(&*state.job(job_id)?, row, column)?.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::write(&path, bytes).map_err(|e| CommandError::new("io", format!("{path}: {e}")))
    })
    .await
    .map_err(|e| CommandError::new("other", e.to_string()))?
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    /// `[row, column]` of each matching cell, row by row.
    matches: Vec<[usize; 2]>,
    /// More cells matched than were reported.
    truncated: bool,
}

/// Case-insensitive substring test; allocation-free when the needle is ASCII.
fn contains_ignore_case(hay: &str, needle: &str, lower_needle: &str) -> bool {
    if needle.is_ascii() {
        let (h, n) = (hay.as_bytes(), needle.as_bytes());
        return n.is_empty() || h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n));
    }
    hay.to_lowercase().contains(lower_needle)
}

fn search(job: &Job, text: &str) -> CommandResult<SearchResult> {
    let (_, batches) = job
        .snapshot()
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    let options = display::format_options();
    let lower = text.to_lowercase();
    let mut matches = Vec::new();
    let mut cell = String::new();
    let mut start = 0;
    for batch in &batches {
        let formatters = batch
            .columns()
            .iter()
            .map(|a| ArrayFormatter::try_new(a.as_ref(), &options))
            .collect::<Result<Vec<_>, _>>()?;
        for row in 0..batch.num_rows() {
            for (col, (array, f)) in batch.columns().iter().zip(&formatters).enumerate() {
                if array.is_null(row) {
                    continue;
                }
                cell.clear();
                let _ = write!(cell, "{}", f.value(row));
                if contains_ignore_case(&cell, text, &lower) {
                    if matches.len() == SEARCH_LIMIT {
                        return Ok(SearchResult {
                            matches,
                            truncated: true,
                        });
                    }
                    matches.push([start + row, col]);
                }
            }
        }
        start += batch.num_rows();
    }
    Ok(SearchResult {
        matches,
        truncated: false,
    })
}

/// The loaded cells whose text contains `text`, ignoring case.
#[tauri::command]
pub async fn search_rows(
    state: State<'_, AppState>,
    job_id: u64,
    text: String,
) -> CommandResult<SearchResult> {
    let job = state.job(job_id)?;
    tauri::async_runtime::spawn_blocking(move || search(&job, &text))
        .await
        .map_err(|e| CommandError::new("other", e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::contains_ignore_case;

    #[test]
    fn searches_ignoring_case() {
        assert!(contains_ignore_case("Hello World", "WORLD", "world"));
        assert!(!contains_ignore_case("Hello", "xyz", "xyz"));
        assert!(contains_ignore_case("ÀBC", "àb", "àb"));
        assert!(contains_ignore_case("abc", "", ""));
    }
}
