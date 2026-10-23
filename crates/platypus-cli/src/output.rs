//! Result writers. CSV and NDJSON stream batch by batch; the table format buffers its
//! (row-limited) input to compute column widths.

use std::io::Write;
use std::sync::Arc;

use arrow_array::builder::StringBuilder;
use arrow_array::{Array, ArrayRef, RecordBatch};
use arrow_cast::display::{ArrayFormatter, FormatOptions};
use arrow_schema::{ArrowError, DataType, Field, Schema, SchemaRef};
use platypus_core::Error;

#[derive(Debug, Clone, Copy)]
pub enum Format {
    Table,
    Csv,
    Ndjson,
}

pub struct Sink {
    kind: SinkKind,
    /// Schema after converting types the writers cannot handle to text.
    schema: SchemaRef,
}

enum SinkKind {
    Table {
        batches: Vec<RecordBatch>,
        out: Box<dyn Write + Send>,
    },
    Csv(Box<arrow_csv::Writer<Box<dyn Write + Send>>>),
    Ndjson(arrow_json::LineDelimitedWriter<Box<dyn Write + Send>>),
}

/// Types the CSV and JSON writers do not support natively are rendered as text.
fn needs_text(data_type: &DataType) -> bool {
    matches!(data_type, DataType::Binary | DataType::Interval(_))
}

/// Renders an array with Arrow's display rules (hex for binary, ISO-like for intervals).
fn to_text(array: &ArrayRef) -> Result<ArrayRef, ArrowError> {
    let formatter = ArrayFormatter::try_new(array.as_ref(), &FormatOptions::default())?;
    let mut builder = StringBuilder::with_capacity(array.len(), array.len() * 16);
    for i in 0..array.len() {
        if array.is_null(i) {
            builder.append_null();
        } else {
            builder.append_value(formatter.value(i).to_string());
        }
    }
    Ok(Arc::new(builder.finish()))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn io_error(e: std::io::Error) -> Error {
    Error::Other(format!("write failed: {e}"))
}

impl Sink {
    pub fn new(format: Format, schema: &Schema, out: Box<dyn Write + Send>) -> Self {
        let fields: Vec<Field> = schema
            .fields()
            .iter()
            .map(|f| {
                if needs_text(f.data_type()) {
                    f.as_ref().clone().with_data_type(DataType::Utf8)
                } else {
                    f.as_ref().clone()
                }
            })
            .collect();
        let kind = match format {
            Format::Table => SinkKind::Table {
                batches: Vec::new(),
                out,
            },
            Format::Csv => SinkKind::Csv(Box::new(
                arrow_csv::WriterBuilder::new().with_header(true).build(out),
            )),
            Format::Ndjson => SinkKind::Ndjson(arrow_json::LineDelimitedWriter::new(out)),
        };
        Self {
            kind,
            schema: Arc::new(Schema::new(fields)),
        }
    }

    pub fn write(&mut self, batch: &RecordBatch) -> Result<(), Error> {
        let columns = batch
            .columns()
            .iter()
            .map(|c| {
                if needs_text(c.data_type()) {
                    to_text(c)
                } else {
                    Ok(Arc::clone(c))
                }
            })
            .collect::<Result<Vec<ArrayRef>, _>>()?;
        let batch = RecordBatch::try_new(Arc::clone(&self.schema), columns)?;
        match &mut self.kind {
            SinkKind::Table { batches, .. } => batches.push(batch),
            SinkKind::Csv(w) => w.write(&batch)?,
            SinkKind::Ndjson(w) => w.write(&batch)?,
        }
        Ok(())
    }

    pub fn finish(self) -> Result<(), Error> {
        match self.kind {
            SinkKind::Table { batches, mut out } => {
                let table = arrow_cast::pretty::pretty_format_batches(&batches)?;
                writeln!(out, "{table}").map_err(io_error)?;
                let rows: usize = batches.iter().map(RecordBatch::num_rows).sum();
                writeln!(out, "({rows} row{})", if rows == 1 { "" } else { "s" })
                    .map_err(io_error)?;
                out.flush().map_err(io_error)
            }
            SinkKind::Csv(w) => w.into_inner().flush().map_err(io_error),
            SinkKind::Ndjson(mut w) => {
                w.finish()?;
                w.into_inner().flush().map_err(io_error)
            }
        }
    }
}
