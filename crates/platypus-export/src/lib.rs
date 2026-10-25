//! Writes Arrow record batches to CSV, JSON, NDJSON or Parquet files, one batch at a time,
//! so results of any size are exported with flat memory.
//!
//! Output goes to `<file>.part` and is renamed into place by [`Exporter::finish`]; an
//! exporter dropped before that (cancelled or failed export) deletes the partial file.
//! [`import`] goes the other way: rows of a data file into a table.

pub mod import;
mod text;

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::builder::StringBuilder;
use arrow_array::{Array, ArrayRef, RecordBatch};
use arrow_cast::display::{ArrayFormatter, FormatOptions};
use arrow_json::writer::{JsonArray, LineDelimited};
use arrow_schema::{ArrowError, DataType, Field, Schema, SchemaRef};
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::errors::ParquetError;
use parquet::file::properties::WriterProperties;
use platypus_core::DriverKind;
use platypus_sql::Dialect;
use serde::Deserialize;

/// Rows per Parquet row group: large enough to compress well, small enough to bound the
/// memory the writer buffers.
const ROW_GROUP_ROWS: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Csv,
    /// One JSON array of objects.
    Json,
    /// One JSON object per line.
    Ndjson,
    Parquet,
    /// `INSERT` statements, one per row.
    Sql,
    Markdown,
    Html,
    Xml,
    Xlsx,
}

impl Format {
    /// The usual file extension.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Ndjson => "ndjson",
            Self::Parquet => "parquet",
            Self::Sql => "sql",
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Xml => "xml",
            Self::Xlsx => "xlsx",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    pub format: Format,
    /// CSV field separator.
    pub delimiter: char,
    /// CSV header row.
    pub header: bool,
    /// SQL: the table the statements insert into, as typed (it is quoted when needed).
    pub table: String,
    /// SQL: the database the statements are written for.
    pub driver: Option<DriverKind>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: Format::Csv,
            delimiter: ',',
            header: true,
            table: "export".to_owned(),
            driver: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Arrow(#[from] ArrowError),
    #[error(transparent)]
    Parquet(#[from] ParquetError),
    #[error("invalid export options: {0}")]
    Options(String),
}

pub type Result<T, E = ExportError> = std::result::Result<T, E>;

enum Sink {
    Csv(Box<arrow_csv::Writer<BufWriter<File>>>),
    Json(arrow_json::Writer<BufWriter<File>, JsonArray>),
    Ndjson(arrow_json::Writer<BufWriter<File>, LineDelimited>),
    Parquet(Box<ArrowWriter<BufWriter<File>>>),
    Text(Box<text::TextSink>),
}

/// Whether a column of `data_type` is written as text in `format`.
fn as_text(data_type: &DataType, format: Format) -> bool {
    use DataType as T;
    match format {
        // The CSV writer handles scalars, not binary or nested values.
        Format::Csv => matches!(
            data_type,
            T::Binary
                | T::LargeBinary
                | T::BinaryView
                | T::FixedSizeBinary(_)
                | T::List(_)
                | T::LargeList(_)
                | T::Struct(_)
                | T::Map(..)
        ),
        // JSON keeps numbers, booleans and strings; decimals become strings so no digit is
        // lost, and dates, times and bytes get their display form.
        Format::Json | Format::Ndjson => !matches!(
            data_type,
            T::Null
                | T::Boolean
                | T::Int8
                | T::Int16
                | T::Int32
                | T::Int64
                | T::UInt8
                | T::UInt16
                | T::UInt32
                | T::UInt64
                | T::Float16
                | T::Float32
                | T::Float64
                | T::Utf8
                | T::LargeUtf8
                | T::Utf8View
        ),
        Format::Parquet => matches!(data_type, T::Interval(_) | T::Duration(_)),
        // Written from the values' text, with their types to tell numbers.
        Format::Sql | Format::Markdown | Format::Html | Format::Xml | Format::Xlsx => false,
    }
}

fn output_schema(schema: &Schema, format: Format) -> SchemaRef {
    Arc::new(Schema::new(
        schema
            .fields()
            .iter()
            .map(|f| {
                if as_text(f.data_type(), format) {
                    Field::new(f.name(), DataType::Utf8, true).with_metadata(f.metadata().clone())
                } else {
                    f.as_ref().clone()
                }
            })
            .collect::<Vec<_>>(),
    ))
}

fn to_text(array: &dyn Array) -> Result<ArrayRef, ArrowError> {
    let formatter = ArrayFormatter::try_new(array, &FormatOptions::default())?;
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

pub struct Exporter {
    sink: Option<Sink>,
    schema: SchemaRef,
    format: Format,
    partial: PathBuf,
    target: PathBuf,
    rows: u64,
}

impl std::fmt::Debug for Exporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Exporter")
            .field("format", &self.format)
            .field("target", &self.target)
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> ExportError + '_ {
    move |source| ExportError::Io {
        path: path.to_owned(),
        source,
    }
}

impl Exporter {
    /// Starts writing a result with `schema` to `path`.
    pub fn create(path: &Path, schema: &Schema, options: &ExportOptions) -> Result<Self> {
        let delimiter = u8::try_from(options.delimiter)
            .ok()
            .filter(u8::is_ascii)
            .ok_or_else(|| {
                ExportError::Options(format!(
                    "the delimiter must be one ASCII character, not {:?}",
                    options.delimiter
                ))
            })?;
        let mut partial = path.as_os_str().to_owned();
        partial.push(".part");
        let partial = PathBuf::from(partial);
        let file = File::create(&partial).map_err(io_error(&partial))?;
        let out = BufWriter::with_capacity(1 << 20, file);
        let schema = output_schema(schema, options.format);

        let sink = match options.format {
            Format::Csv => Sink::Csv(Box::new(
                arrow_csv::WriterBuilder::new()
                    .with_delimiter(delimiter)
                    .with_header(options.header)
                    .with_null(String::new())
                    .build(out),
            )),
            Format::Json => Sink::Json(
                arrow_json::WriterBuilder::new()
                    .with_explicit_nulls(true)
                    .build::<_, JsonArray>(out),
            ),
            Format::Ndjson => Sink::Ndjson(
                arrow_json::WriterBuilder::new()
                    .with_explicit_nulls(true)
                    .build::<_, LineDelimited>(out),
            ),
            format @ (Format::Sql
            | Format::Markdown
            | Format::Html
            | Format::Xml
            | Format::Xlsx) => {
                let dialect = Dialect::from(options.driver.unwrap_or(DriverKind::Postgres));
                let table = dialect.qualified(None, &options.table);
                Sink::Text(Box::new(text::TextSink::new(
                    format,
                    &schema,
                    out,
                    (dialect, table),
                )?))
            }
            Format::Parquet => {
                let properties = WriterProperties::builder()
                    .set_compression(Compression::ZSTD(ZstdLevel::try_new(3)?))
                    .set_max_row_group_row_count(Some(ROW_GROUP_ROWS))
                    .build();
                Sink::Parquet(Box::new(ArrowWriter::try_new(
                    out,
                    Arc::clone(&schema),
                    Some(properties),
                )?))
            }
        };
        Ok(Self {
            sink: Some(sink),
            schema,
            format: options.format,
            partial,
            target: path.to_owned(),
            rows: 0,
        })
    }

    pub fn write(&mut self, batch: &RecordBatch) -> Result<()> {
        let columns = batch
            .columns()
            .iter()
            .zip(self.schema.fields())
            .map(|(column, field)| {
                if column.data_type() == field.data_type() {
                    Ok(Arc::clone(column))
                } else {
                    to_text(column.as_ref())
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let batch = RecordBatch::try_new(Arc::clone(&self.schema), columns)?;
        match self.sink.as_mut() {
            Some(Sink::Csv(w)) => w.write(&batch)?,
            Some(Sink::Json(w)) => w.write(&batch)?,
            Some(Sink::Ndjson(w)) => w.write(&batch)?,
            Some(Sink::Parquet(w)) => w.write(&batch)?,
            Some(Sink::Text(w)) => w.write(&batch)?,
            None => {}
        }
        self.rows += batch.num_rows() as u64;
        Ok(())
    }

    #[must_use]
    pub fn rows(&self) -> u64 {
        self.rows
    }

    /// Completes the file and moves it into place; returns the number of rows written.
    pub fn finish(mut self) -> Result<u64> {
        let out = match self.sink.take() {
            Some(Sink::Csv(w)) => w.into_inner(),
            Some(Sink::Json(mut w)) => {
                w.finish()?;
                w.into_inner()
            }
            Some(Sink::Ndjson(mut w)) => {
                w.finish()?;
                w.into_inner()
            }
            Some(Sink::Parquet(w)) => w.into_inner()?,
            Some(Sink::Text(w)) => {
                w.finish(&self.partial)?;
                return self.into_place();
            }
            None => return Ok(self.rows),
        };
        let file = out
            .into_inner()
            .map_err(|e| io_error(&self.partial)(e.into_error()))?;
        file.sync_all().map_err(io_error(&self.partial))?;
        drop(file);
        self.into_place()
    }

    /// Moves the completed file into place.
    fn into_place(self) -> Result<u64> {
        std::fs::rename(&self.partial, &self.target).map_err(io_error(&self.target))?;
        Ok(self.rows)
    }
}

impl Drop for Exporter {
    fn drop(&mut self) {
        if let Some(sink) = self.sink.take() {
            drop(sink);
            let _ = std::fs::remove_file(&self.partial);
        }
    }
}

#[cfg(test)]
mod tests {
    use arrow_array::{
        BinaryArray, Decimal128Array, Int64Array, StringArray, TimestampMicrosecondArray,
    };
    use arrow_schema::TimeUnit;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    use super::*;

    fn sample() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("price", DataType::Decimal128(10, 2), true),
            Field::new("name", DataType::Utf8, true),
            Field::new("raw", DataType::Binary, true),
            Field::new(
                "at",
                DataType::Timestamp(TimeUnit::Microsecond, Some("+00:00".into())),
                true,
            ),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int64Array::from(vec![1, 2])),
                Arc::new(
                    Decimal128Array::from(vec![Some(12_345), None])
                        .with_precision_and_scale(10, 2)
                        .unwrap(),
                ),
                Arc::new(StringArray::from(vec![Some("a,\"b\""), None])),
                Arc::new(BinaryArray::from(vec![Some(&[0xde, 0xad][..]), None])),
                Arc::new(
                    TimestampMicrosecondArray::from(vec![Some(1_000_000), None])
                        .with_timezone("+00:00"),
                ),
            ],
        )
        .unwrap()
    }

    fn export(dir: &Path, name: &str, options: &ExportOptions) -> PathBuf {
        let path = dir.join(name);
        let batch = sample();
        let mut exporter = Exporter::create(&path, &batch.schema(), options).unwrap();
        exporter.write(&batch).unwrap();
        exporter.write(&batch).unwrap();
        assert_eq!(exporter.finish().unwrap(), 4);
        assert!(!dir.join(format!("{name}.part")).exists());
        path
    }

    #[test]
    fn writes_csv() {
        let dir = tempfile::tempdir().unwrap();
        let options = ExportOptions {
            delimiter: ';',
            ..ExportOptions::default()
        };
        let text = std::fs::read_to_string(export(dir.path(), "out.csv", &options)).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines[0], "id;price;name;raw;at");
        assert_eq!(
            lines[1],
            "1;123.45;\"a,\"\"b\"\"\";dead;1970-01-01T00:00:01Z"
        );
        assert_eq!(lines[2], "2;;;;");
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn writes_json_and_ndjson() {
        let dir = tempfile::tempdir().unwrap();
        let json = ExportOptions {
            format: Format::Json,
            ..ExportOptions::default()
        };
        let text = std::fs::read_to_string(export(dir.path(), "out.json", &json)).unwrap();
        assert!(text.starts_with("[{\"id\":1,\"price\":\"123.45\",\"name\":\"a,\\\"b\\\"\""));
        assert!(text.contains("\"raw\":null"), "explicit nulls: {text}");
        assert!(text.trim_end().ends_with(']'));

        let ndjson = ExportOptions {
            format: Format::Ndjson,
            ..ExportOptions::default()
        };
        let text = std::fs::read_to_string(export(dir.path(), "out.ndjson", &ndjson)).unwrap();
        assert_eq!(text.lines().count(), 4);
        assert!(text.lines().next().unwrap().contains("\"raw\":\"dead\""));
    }

    #[test]
    fn writes_parquet_with_native_types() {
        let dir = tempfile::tempdir().unwrap();
        let options = ExportOptions {
            format: Format::Parquet,
            ..ExportOptions::default()
        };
        let file = File::open(export(dir.path(), "out.parquet", &options)).unwrap();
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap();
        let batches: Vec<_> = reader.map(Result::unwrap).collect();
        assert_eq!(batches.iter().map(RecordBatch::num_rows).sum::<usize>(), 4);
        let schema = batches[0].schema();
        assert_eq!(schema.field(1).data_type(), &DataType::Decimal128(10, 2));
        assert_eq!(schema.field(3).data_type(), &DataType::Binary);
    }

    #[test]
    fn abandoned_exports_leave_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.csv");
        let batch = sample();
        let mut exporter =
            Exporter::create(&path, &batch.schema(), &ExportOptions::default()).unwrap();
        exporter.write(&batch).unwrap();
        drop(exporter);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn rejects_bad_delimiters() {
        let dir = tempfile::tempdir().unwrap();
        let options = ExportOptions {
            delimiter: '§',
            ..ExportOptions::default()
        };
        let err = Exporter::create(&dir.path().join("x.csv"), &sample().schema(), &options);
        assert!(matches!(err, Err(ExportError::Options(_))));
    }
    fn export_as(dir: &Path, name: &str, format: Format) -> String {
        let options = ExportOptions {
            format,
            table: "Items".to_owned(),
            driver: Some(DriverKind::MySql),
            ..ExportOptions::default()
        };
        std::fs::read_to_string(export(dir, name, &options)).unwrap()
    }

    #[test]
    fn writes_sql_markdown_html_and_xml() {
        let dir = tempfile::tempdir().unwrap();
        let sql = export_as(dir.path(), "out.sql", Format::Sql);
        let lines: Vec<_> = sql.lines().collect();
        assert_eq!(
            lines[0],
            "INSERT INTO Items (id, price, name, raw, `at`) VALUES (1, 123.45, 'a,\"b\"', X'dead', '1970-01-01T00:00:01');"
        );
        assert_eq!(
            lines[1],
            "INSERT INTO Items (id, price, name, raw, `at`) VALUES (2, NULL, NULL, NULL, NULL);"
        );
        assert_eq!(lines.len(), 4);

        let md = export_as(dir.path(), "out.md", Format::Markdown);
        let lines: Vec<_> = md.lines().collect();
        assert_eq!(lines[0], "| id | price | name | raw | at |");
        assert_eq!(lines[1], "| --- | --- | --- | --- | --- |");
        assert_eq!(
            lines[2],
            "| 1 | 123.45 | a,\"b\" | dead | 1970-01-01T00:00:01Z |"
        );
        assert_eq!(lines[3], "| 2 |  |  |  |  |");

        let html = export_as(dir.path(), "out.html", Format::Html);
        assert!(html.contains("<th>id</th><th>price</th>"), "{html}");
        assert!(html.contains("<td>a,&quot;b&quot;</td>"), "{html}");
        assert!(html.trim_end().ends_with("</html>"), "{html}");

        let xml = export_as(dir.path(), "out.xml", Format::Xml);
        assert!(xml.starts_with("<?xml"), "{xml}");
        assert!(
            xml.contains("<field name=\"name\">a,&quot;b&quot;</field>"),
            "{xml}"
        );
        assert!(
            xml.contains("<field name=\"name\" null=\"true\"/>"),
            "{xml}"
        );
        assert_eq!(xml.matches("<row>").count(), 4);
    }

    #[test]
    fn writes_xlsx() {
        use calamine::{Data, Reader};
        let dir = tempfile::tempdir().unwrap();
        let options = ExportOptions {
            format: Format::Xlsx,
            ..ExportOptions::default()
        };
        let path = export(dir.path(), "out.xlsx", &options);
        let mut book = calamine::open_workbook_auto(&path).unwrap();
        let sheet = book.worksheet_range_at(0).unwrap().unwrap();
        let rows: Vec<_> = sheet.rows().collect();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0][0], Data::String("id".into()));
        assert_eq!(rows[1][0], Data::Float(1.0));
        assert_eq!(rows[1][1], Data::Float(123.45));
        assert_eq!(rows[1][2], Data::String("a,\"b\"".into()));
        assert_eq!(rows[2][2], Data::Empty);
    }
}
