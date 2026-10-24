//! Converts result windows into the grid's wire format: every column rendered to text in
//! Rust (one set of formatting rules for grid, CLI and exports), serialized as Arrow IPC.
//! The grid's text also follows the user's [`DisplayFormat`].

use std::sync::Arc;

use arrow_array::builder::StringBuilder;
use arrow_array::{Array, ArrayRef, RecordBatch};
use arrow_cast::display::{ArrayFormatter, FormatOptions};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{ArrowError, DataType, Field, Schema};

use crate::formats::{self, DisplayFormat};

/// Longer values are cut for the grid; the full value is fetched on demand.
const MAX_CELL_CHARS: usize = 500;

/// The plain formats: what editing, copying and exports read and write.
pub fn format_options() -> FormatOptions<'static> {
    FormatOptions::default()
        .with_date_format(Some(formats::DATE_FORMAT))
        .with_timestamp_format(Some(formats::TIMESTAMP_FORMAT))
        .with_timestamp_tz_format(Some("%Y-%m-%d %H:%M:%S%.f%:z"))
}

/// One line of a grid cell: cut, without control characters, which would break the line.
fn grid_cell(cell: &str) -> String {
    let mut cell = match cell.char_indices().nth(MAX_CELL_CHARS) {
        Some((cut, _)) => format!("{}…", &cell[..cut]),
        None => cell.to_owned(),
    };
    if cell.contains(['\n', '\r', '\t']) {
        cell = cell.replace(['\n', '\r', '\t'], " ");
    }
    cell
}

fn render(array: &ArrayRef, format: &DisplayFormat) -> Result<ArrayRef, ArrowError> {
    let mut builder = StringBuilder::with_capacity(array.len(), array.len() * 16);
    if format.own_timestamps(array.data_type()) {
        let mut cells = vec![None; array.len()];
        format.timestamps(array, |i, text| cells[i] = Some(text.to_owned()))?;
        for cell in cells {
            builder.append_option(cell);
        }
        return Ok(Arc::new(builder.finish()));
    }
    let options = FormatOptions::default()
        .with_date_format(Some(&format.date))
        .with_timestamp_format(Some(&format.timestamp))
        .with_timestamp_tz_format(Some(&format.timestamp_tz));
    let formatter = ArrayFormatter::try_new(array.as_ref(), &options)?;
    let numeric = array.data_type().is_numeric();
    let mut cell = String::new();
    for i in 0..array.len() {
        if array.is_null(i) {
            builder.append_null();
            continue;
        }
        cell.clear();
        std::fmt::write(&mut cell, format_args!("{}", formatter.value(i)))
            .map_err(|e| ArrowError::ComputeError(e.to_string()))?;
        match format.number(&cell).filter(|_| numeric) {
            Some(grouped) => builder.append_value(grouped),
            None => builder.append_value(grid_cell(&cell)),
        }
    }
    Ok(Arc::new(builder.finish()))
}

/// Appends the full text of every row of `batch` to `out`: uncut, line breaks kept, `None`
/// for NULL. The data editor edits these values and identifies rows by them.
pub fn rows_text(
    batch: &RecordBatch,
    out: &mut Vec<Vec<Option<String>>>,
) -> Result<(), ArrowError> {
    let options = format_options();
    // One formatter per column, not per value.
    let formatters = batch
        .columns()
        .iter()
        .map(|array| ArrayFormatter::try_new(array.as_ref(), &options))
        .collect::<Result<Vec<_>, _>>()?;
    for row in 0..batch.num_rows() {
        out.push(
            batch
                .columns()
                .iter()
                .zip(&formatters)
                .map(|(array, f)| (!array.is_null(row)).then(|| f.value(row).to_string()))
                .collect(),
        );
    }
    Ok(())
}

/// Renders `batches` (consecutive slices of one result) in `format` and writes them as an IPC
/// stream.
pub fn to_ipc(
    schema: &Schema,
    batches: &[RecordBatch],
    format: &DisplayFormat,
) -> Result<Vec<u8>, ArrowError> {
    let text_schema = Arc::new(Schema::new(
        schema
            .fields()
            .iter()
            .map(|f| Field::new(f.name(), DataType::Utf8, true))
            .collect::<Vec<_>>(),
    ));
    let mut writer = StreamWriter::try_new(Vec::new(), &text_schema)?;
    for batch in batches {
        let columns = batch
            .columns()
            .iter()
            .map(|c| render(c, format))
            .collect::<Result<Vec<_>, _>>()?;
        writer.write(&RecordBatch::try_new(Arc::clone(&text_schema), columns)?)?;
    }
    writer.into_inner()
}
