//! Converts result windows into the grid's wire format: every column rendered to text in
//! Rust (one set of formatting rules for grid, CLI and exports), serialized as Arrow IPC.

use std::sync::Arc;

use arrow_array::builder::StringBuilder;
use arrow_array::{Array, ArrayRef, RecordBatch};
use arrow_cast::display::{ArrayFormatter, FormatOptions};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{ArrowError, DataType, Field, Schema};

/// Longer values are cut for the grid; the full value is fetched on demand.
const MAX_CELL_CHARS: usize = 500;

pub fn format_options() -> FormatOptions<'static> {
    FormatOptions::default()
        .with_date_format(Some("%Y-%m-%d"))
        .with_timestamp_format(Some("%Y-%m-%d %H:%M:%S%.f"))
        .with_timestamp_tz_format(Some("%Y-%m-%d %H:%M:%S%.f%:z"))
}

fn render(array: &ArrayRef, options: &FormatOptions<'_>) -> Result<ArrayRef, ArrowError> {
    let formatter = ArrayFormatter::try_new(array.as_ref(), options)?;
    let mut builder = StringBuilder::with_capacity(array.len(), array.len() * 16);
    let mut cell = String::new();
    for i in 0..array.len() {
        if array.is_null(i) {
            builder.append_null();
            continue;
        }
        cell.clear();
        std::fmt::write(&mut cell, format_args!("{}", formatter.value(i)))
            .map_err(|e| ArrowError::ComputeError(e.to_string()))?;
        if let Some((cut, _)) = cell.char_indices().nth(MAX_CELL_CHARS) {
            cell.truncate(cut);
            cell.push('…');
        }
        // Control characters would break the single-line grid cells.
        if cell.contains(['\n', '\r', '\t']) {
            cell = cell.replace(['\n', '\r', '\t'], " ");
        }
        builder.append_value(&cell);
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

/// Renders `batches` (consecutive slices of one result) and writes them as an IPC stream.
pub fn to_ipc(schema: &Schema, batches: &[RecordBatch]) -> Result<Vec<u8>, ArrowError> {
    let options = format_options();
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
            .map(|c| render(c, &options))
            .collect::<Result<Vec<_>, _>>()?;
        writer.write(&RecordBatch::try_new(Arc::clone(&text_schema), columns)?)?;
    }
    writer.into_inner()
}
