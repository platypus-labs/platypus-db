//! Grid features on loaded results: copying a selection in other formats, searching the
//! loaded rows, the value panel and quick filters. They read the job's buffered rows, so they
//! never touch the session.

use std::fmt::Write as _;

use arrow_array::Array;
use arrow_array::cast::AsArray;
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

/// The bytes of `row` in a binary array; `None` when the array is not binary.
pub(super) fn binary_at(array: &dyn Array, row: usize) -> Option<&[u8]> {
    if let Some(a) = array.as_binary_opt::<i32>() {
        Some(a.value(row))
    } else if let Some(a) = array.as_binary_opt::<i64>() {
        Some(a.value(row))
    } else if let Some(a) = array.as_binary_view_opt() {
        Some(a.value(row))
    } else {
        array.as_fixed_size_binary_opt().map(|a| a.value(row))
    }
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
    let bytes = if let Some(b) = binary_at(array.as_ref(), 0) {
        b.to_vec()
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

/// A file's bytes as the data editor writes a cell: hex for a binary column (the text the
/// grid shows and saving turns back into bytes), else the text, which must be UTF-8.
fn file_cell_text(bytes: Vec<u8>, binary: bool) -> CommandResult<String> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    if binary {
        let mut hex = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            hex.push(char::from(HEX[usize::from(b >> 4)]));
            hex.push(char::from(HEX[usize::from(b & 0x0f)]));
        }
        return Ok(hex);
    }
    let mut text = String::from_utf8(bytes).map_err(|e| {
        CommandError::new(
            "invalid_utf8",
            format!(
                "the file is not UTF-8 text (invalid byte at offset {})",
                e.utf8_error().valid_up_to()
            ),
        )
    })?;
    // A byte order mark says how the file is encoded; it is not part of the text.
    if text.starts_with('\u{feff}') {
        text.drain(..'\u{feff}'.len_utf8());
    }
    Ok(text)
}

/// Files larger than this are not loaded into a cell: as hex, a larger binary file would not
/// fit in one string of the webview, and every copy on the way takes as much memory again.
/// Matches `MAX_CELL_FILE` of the frontend.
const MAX_CELL_FILE: u64 = 200 * 1024 * 1024;

/// The bytes of the regular file at `path`, refused above `cap` bytes before reading, and
/// read no further than `cap` in case the file grows meanwhile.
fn read_capped(path: &str, cap: u64) -> CommandResult<Vec<u8>> {
    use std::io::Read as _;
    let io = |e: std::io::Error| CommandError::new("io", format!("{path}: {e}"));
    let too_large = |size: u64| {
        CommandError::new(
            "too_large",
            format!(
                "{path}: the file is {} MB, more than the {} MB a cell can load",
                size.div_ceil(1024 * 1024),
                cap / (1024 * 1024)
            ),
        )
    };
    let file = std::fs::File::open(path).map_err(io)?;
    let meta = file.metadata().map_err(io)?;
    if !meta.is_file() {
        return Err(CommandError::new(
            "io",
            format!("{path}: not a regular file"),
        ));
    }
    if meta.len() > cap {
        return Err(too_large(meta.len()));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    file.take(cap + 1).read_to_end(&mut bytes).map_err(io)?;
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if read > cap {
        return Err(too_large(read));
    }
    Ok(bytes)
}

/// The size of a file in bytes, so the UI can warn before loading a large one.
#[tauri::command]
pub async fn file_size(path: String) -> CommandResult<u64> {
    tokio::fs::metadata(&path)
        .await
        .map(|m| m.len())
        .map_err(|e| CommandError::new("io", format!("{path}: {e}")))
}

/// A file's contents for a cell of the data editor: hex for `binary` columns, else UTF-8
/// text. The value becomes a pending change like a typed one. Files above `MAX_CELL_FILE`
/// are refused.
#[tauri::command]
pub async fn read_cell_file(path: String, binary: bool) -> CommandResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        file_cell_text(read_capped(&path, MAX_CELL_FILE)?, binary)
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
        let schema = batch.schema();
        let formatters = schema
            .fields()
            .iter()
            .zip(batch.columns())
            .map(|(f, a)| platypus_core::special::column_formatter(f, a.as_ref(), &options))
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
    use std::io::Write as _;

    use super::{contains_ignore_case, file_cell_text, read_capped};

    #[test]
    fn searches_ignoring_case() {
        assert!(contains_ignore_case("Hello World", "WORLD", "world"));
        assert!(!contains_ignore_case("Hello", "xyz", "xyz"));
        assert!(contains_ignore_case("ÀBC", "àb", "àb"));
        assert!(contains_ignore_case("abc", "", ""));
    }

    #[test]
    fn binary_files_load_as_hex() {
        let hex = file_cell_text(vec![0x00, 0x0f, 0xa5, 0xff], true).expect("hex");
        assert_eq!(hex, "000fa5ff");
        assert_eq!(file_cell_text(Vec::new(), true).expect("empty"), "");
        // Bytes that are not UTF-8 are fine in a binary column.
        assert_eq!(file_cell_text(vec![0xc3, 0x28], true).expect("hex"), "c328");
    }

    #[test]
    fn text_files_must_be_utf8() {
        let text = file_cell_text("héllo\nworld".as_bytes().to_vec(), false).expect("text");
        assert_eq!(text, "héllo\nworld");
        let error = file_cell_text(vec![b'a', b'b', 0xc3, 0x28], false).expect_err("not UTF-8");
        let error = serde_json::to_value(&error).expect("json");
        assert_eq!(error["kind"], "invalid_utf8");
        let message = error["message"].as_str().unwrap_or_default();
        assert!(message.contains("offset 2"), "{message}");
    }

    #[test]
    fn text_files_lose_their_byte_order_mark() {
        let text = file_cell_text(vec![0xef, 0xbb, 0xbf, b'a', b'b', b'c'], false).expect("text");
        assert_eq!(text, "abc");
    }

    #[test]
    fn files_above_the_cap_are_refused_before_reading() {
        let mut file = tempfile::NamedTempFile::new().expect("file");
        file.write_all(b"0123456789").expect("write");
        let path = file.path().to_string_lossy().into_owned();
        assert_eq!(read_capped(&path, 10).expect("at the cap"), b"0123456789");
        let error = read_capped(&path, 9).expect_err("above the cap");
        let error = serde_json::to_value(&error).expect("json");
        assert_eq!(error["kind"], "too_large");
        let dir = tempfile::tempdir().expect("dir");
        let dir = dir.path().to_string_lossy().into_owned();
        assert!(read_capped(&dir, 10).is_err(), "a directory is not a file");
    }
}
