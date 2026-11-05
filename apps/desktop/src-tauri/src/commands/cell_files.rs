//! Files held by cells: which cells are files (by their bytes, never by the column's type),
//! saving one, and saving the files of a column to a folder. A binary cell is a file when its
//! first bytes say so; a text cell, when it holds a `data:` URL, base64 or hex that decodes to
//! one. Only the start of a cell is read to tell, so a menu can ask for many at once.

use std::fmt::Write as _;
use std::io::Write as _;

use arrow_array::cast::AsArray;
use arrow_array::{Array, ArrayRef};
use serde::Serialize;
use tauri::State;

use super::AppState;
use super::grid::binary_at;
use crate::display;
use crate::error::{CommandError, CommandResult};
use crate::filetype::{self, FileType};
use crate::jobs::Job;

/// What a cell holds, as it is stored.
enum Cell<'a> {
    Bytes(&'a [u8]),
    Text(&'a str),
}

/// The text of `row` in a string array; `None` when the array holds no text.
fn text_at(array: &dyn Array, row: usize) -> Option<&str> {
    if let Some(a) = array.as_string_opt::<i32>() {
        Some(a.value(row))
    } else if let Some(a) = array.as_string_opt::<i64>() {
        Some(a.value(row))
    } else {
        array.as_string_view_opt().map(|a| a.value(row))
    }
}

/// The stored value of `row`; `None` for NULL and for types that hold neither bytes nor text.
fn cell_at(array: &dyn Array, row: usize) -> Option<Cell<'_>> {
    if array.is_null(row) {
        return None;
    }
    binary_at(array, row)
        .map(Cell::Bytes)
        .or_else(|| text_at(array, row).map(Cell::Text))
}

/// A file found in a cell.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CellFile {
    #[serde(flatten)]
    kind: FileType,
    /// The bytes of the file (decoded, for a text cell).
    size: u64,
    /// The cell is text that holds the file coded; saving must decode it.
    coded: bool,
}

/// The file `array[row]` holds, if any.
fn file_at(array: &dyn Array, row: usize) -> Option<CellFile> {
    match cell_at(array, row)? {
        Cell::Bytes(b) => Some(CellFile {
            kind: filetype::detect(&b[..b.len().min(filetype::PREFIX)])?,
            size: b.len() as u64,
            coded: false,
        }),
        Cell::Text(t) => filetype::text_file(t).map(|f| CellFile {
            kind: f.kind,
            size: f.size,
            coded: true,
        }),
    }
}

/// The bytes of the file `array[row]` holds: a binary value as stored, a text value decoded.
fn file_bytes(array: &dyn Array, row: usize) -> Option<Vec<u8>> {
    match cell_at(array, row)? {
        Cell::Bytes(b) => Some(b.to_vec()),
        Cell::Text(t) => filetype::decode_text(t),
    }
}

fn no_result() -> CommandError {
    CommandError::new("not_ready", "query has no result set yet")
}

/// Column `column` of loaded rows `offset..offset + limit`, one array per batch.
fn column_window(
    job: &Job,
    offset: usize,
    limit: usize,
    column: usize,
) -> CommandResult<Vec<ArrayRef>> {
    let (schema, batches) = job.window(offset, limit).ok_or_else(no_result)?;
    if column >= schema.fields().len() {
        return Err(CommandError::not_found("column", column + 1));
    }
    Ok(batches.iter().map(|b| b.column(column).clone()).collect())
}

/// The file the cell holds; `None` for NULL or any other value.
#[tauri::command]
pub fn cell_file(
    state: State<'_, AppState>,
    job_id: u64,
    row: usize,
    column: usize,
) -> CommandResult<Option<CellFile>> {
    let job = state.job(job_id)?;
    let arrays = column_window(&job, row, 1, column)?;
    Ok(arrays.first().and_then(|a| file_at(a.as_ref(), 0)))
}

/// For each loaded row `offset..offset + limit`, the file its cell in `column` holds.
#[tauri::command]
pub fn column_files(
    state: State<'_, AppState>,
    job_id: u64,
    column: usize,
    offset: usize,
    limit: usize,
) -> CommandResult<Vec<Option<CellFile>>> {
    let job = state.job(job_id)?;
    let mut files = Vec::with_capacity(limit.min(job.loaded_rows()));
    for array in column_window(&job, offset, limit, column)? {
        files.extend((0..array.len()).map(|row| file_at(array.as_ref(), row)));
    }
    Ok(files)
}

/// The file bytes of one cell, for [`super::grid::save_cell`] with `decode`.
pub(super) fn decoded_cell(job: &Job, row: usize, column: usize) -> CommandResult<Vec<u8>> {
    let arrays = column_window(job, row, 1, column)?;
    let array = arrays
        .first()
        .ok_or_else(|| CommandError::not_found("row", row + 1))?;
    file_bytes(array.as_ref(), 0)
        .ok_or_else(|| CommandError::new("not_a_file", "the cell does not hold a file"))
}

/// A name Windows (and so every system) accepts for a file: no reserved characters or device
/// names, no trailing dot or space, not too long. `fallback` when nothing is left.
fn safe_stem(value: &str, fallback: &str) -> String {
    let named: String = value
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut stem = named.trim().trim_end_matches(['.', ' ']);
    let cut: String;
    if stem.chars().count() > 100 {
        cut = stem.chars().take(100).collect();
        stem = cut.trim_end_matches(['.', ' ']);
    }
    let stem = stem.to_owned();
    if stem.is_empty() || stem.chars().all(|c| c == '_') {
        return fallback.to_owned();
    }
    let device = stem
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|p| {
            device
                .strip_prefix(p)
                .is_some_and(|n| n.len() == 1 && n != "0" && n.bytes().all(|b| b.is_ascii_digit()))
        });
    if reserved { format!("_{stem}") } else { stem }
}

/// Creates a new file in `folder` named `stem.ext`, or `stem-2.ext`, `stem-3.ext`... when the
/// name is taken; an existing file is never replaced. Returns the file and its path.
fn create_new_in(
    folder: &std::path::Path,
    stem: &str,
    ext: &str,
) -> std::io::Result<(std::fs::File, std::path::PathBuf)> {
    for n in 1u32.. {
        let mut name = stem.to_owned();
        if n > 1 {
            let _ = write!(name, "-{n}");
        }
        let _ = write!(name, ".{ext}");
        let path = folder.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((file, path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("no free name"))
}

/// What saving a run of files did.
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct SavedFiles {
    written: usize,
    /// Cells that hold no file (or NULL): left out.
    skipped: usize,
    failed: usize,
    /// The first reasons for failures.
    errors: Vec<String>,
}

fn save_window(
    job: &Job,
    column: usize,
    offset: usize,
    limit: usize,
    folder: &std::path::Path,
    name_column: Option<usize>,
) -> CommandResult<SavedFiles> {
    let arrays = column_window(job, offset, limit, column)?;
    let (_, batches) = job.window(offset, limit).ok_or_else(no_result)?;
    let mut saved = SavedFiles::default();
    let mut row = offset;
    for (array, batch) in arrays.iter().zip(&batches) {
        let mut names = Vec::new();
        if let Some(c) = name_column {
            display::columns_text(batch, Some(&[c]), &mut names)?;
        }
        for i in 0..array.len() {
            row += 1;
            let Some(file) = file_at(array.as_ref(), i) else {
                saved.skipped += 1;
                continue;
            };
            let value = names
                .get(i)
                .and_then(|r| r.first())
                .and_then(Option::as_deref);
            let stem = safe_stem(value.unwrap_or_default(), &row.to_string());
            let outcome = file_bytes(array.as_ref(), i)
                .ok_or_else(|| "the cell does not decode".to_owned())
                .and_then(|bytes| {
                    let (mut f, path) = create_new_in(folder, &stem, file.kind.ext)
                        .map_err(|e| format!("{stem}.{}: {e}", file.kind.ext))?;
                    f.write_all(&bytes)
                        .map_err(|e| format!("{}: {e}", path.display()))
                });
            match outcome {
                Ok(()) => saved.written += 1,
                Err(e) => {
                    saved.failed += 1;
                    if saved.errors.len() < 5 {
                        saved.errors.push(e);
                    }
                }
            }
        }
    }
    Ok(saved)
}

/// Writes the files of `column` in loaded rows `offset..offset + limit` into `folder`, one per
/// cell, named from the value of `name_column` in the row (the row number without one) and
/// the type's extension, with `-2`, `-3`... after a name already taken. Cells that hold no file
/// are skipped.
#[tauri::command]
pub async fn save_cell_files(
    state: State<'_, AppState>,
    job_id: u64,
    column: usize,
    offset: usize,
    limit: usize,
    folder: String,
    name_column: Option<usize>,
) -> CommandResult<SavedFiles> {
    let job = state.job(job_id)?;
    state.exportable(job.session_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let folder = std::path::PathBuf::from(folder);
        if !folder.is_dir() {
            return Err(CommandError::new(
                "io",
                format!("{}: not a folder", folder.display()),
            ));
        }
        save_window(&job, column, offset, limit, &folder, name_column)
    })
    .await
    .map_err(|e| CommandError::new("other", e.to_string()))?
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, BinaryArray, Int64Array, StringArray};

    use super::{create_new_in, file_at, file_bytes, safe_stem};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().fold(String::new(), |mut s, b| {
            let _ = std::fmt::Write::write_fmt(&mut s, format_args!("{b:02x}"));
            s
        })
    }

    #[test]
    fn binary_cells_are_files_by_their_bytes() {
        let a: ArrayRef = Arc::new(BinaryArray::from_iter([
            Some(PNG),
            Some(b"just bytes".as_slice()),
            None,
        ]));
        let found = file_at(a.as_ref(), 0).expect("png");
        assert_eq!(
            (found.kind.name, found.size, found.coded),
            ("PNG", 16, false)
        );
        assert_eq!(file_bytes(a.as_ref(), 0).as_deref(), Some(PNG));
        assert_eq!(file_at(a.as_ref(), 1), None);
        assert_eq!(file_at(a.as_ref(), 2), None, "NULL");
    }

    #[test]
    fn text_cells_are_files_when_they_decode_to_one() {
        // PostgreSQL prints a bytea as `\x` and hex; a varchar may hold it too.
        let pg = format!("\\x{}", hex(PNG));
        let url = "data:text/plain;base64,aGVsbG8gd29ybGQ=".to_owned();
        let words = "an ordinary sentence in a text column".to_owned();
        let a: ArrayRef = Arc::new(StringArray::from(vec![
            Some(pg),
            Some(url),
            Some(words),
            None,
        ]));
        let png = file_at(a.as_ref(), 0).expect("bytea text");
        assert_eq!((png.kind.name, png.coded), ("PNG", true));
        assert_eq!(file_bytes(a.as_ref(), 0).as_deref(), Some(PNG));
        assert_eq!(file_at(a.as_ref(), 1).expect("data URL").kind.ext, "txt");
        assert_eq!(
            file_bytes(a.as_ref(), 1).as_deref(),
            Some(b"hello world".as_slice())
        );
        assert_eq!(file_at(a.as_ref(), 2), None);
        assert_eq!(
            file_bytes(a.as_ref(), 2),
            None,
            "plain text is never decoded"
        );
        assert_eq!(file_at(a.as_ref(), 3), None);
    }

    #[test]
    fn other_types_are_no_files() {
        let a: ArrayRef = Arc::new(Int64Array::from(vec![1, 2]));
        assert_eq!(file_at(a.as_ref(), 0), None);
    }

    #[test]
    fn names_are_safe_on_windows() {
        assert_eq!(safe_stem("invoice 12", "3"), "invoice 12");
        assert_eq!(
            safe_stem("a/b\\c:d*e?f\"g<h>i|j", "3"),
            "a_b_c_d_e_f_g_h_i_j"
        );
        assert_eq!(safe_stem("report. ", "3"), "report");
        assert_eq!(safe_stem("", "3"), "3");
        assert_eq!(safe_stem("???", "4"), "4");
        assert_eq!(safe_stem("CON", "3"), "_CON");
        assert_eq!(safe_stem("nul.txt", "3"), "_nul.txt");
        assert_eq!(safe_stem("com1", "3"), "_com1");
        assert_eq!(safe_stem("COM0", "3"), "COM0");
        assert_eq!(safe_stem("line\nbreak", "3"), "line_break");
        assert_eq!(safe_stem(&"x".repeat(300), "3").chars().count(), 100);
    }

    #[test]
    fn a_taken_name_gets_a_suffix_and_is_never_overwritten() {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(dir.path().join("a.pdf"), b"old").expect("write");
        let (_, first) = create_new_in(dir.path(), "a", "pdf").expect("second");
        let (_, second) = create_new_in(dir.path(), "a", "pdf").expect("third");
        assert_eq!(first.file_name().and_then(|n| n.to_str()), Some("a-2.pdf"));
        assert_eq!(second.file_name().and_then(|n| n.to_str()), Some("a-3.pdf"));
        assert_eq!(
            std::fs::read(dir.path().join("a.pdf")).expect("read"),
            b"old"
        );
    }
}
