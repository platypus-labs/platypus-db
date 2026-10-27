//! Writes Arrow record batches to CSV, JSON, NDJSON or Parquet files, one batch at a time,
//! so results of any size are exported with flat memory.
//!
//! Output goes to `<file>.part` and is renamed into place by [`Exporter::finish`]; an
//! exporter dropped before that (cancelled or failed export) deletes the partial file.
//! [`import`] goes the other way: rows of a data file into a table.

pub mod fake;
pub mod import;
mod json;
pub mod migrate;
pub mod s3;
mod text;

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::builder::StringBuilder;
use arrow_array::{Array, ArrayRef, RecordBatch};
use arrow_cast::display::FormatOptions;
use arrow_schema::{ArrowError, DataType, Field, Schema, SchemaRef};
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression as ParquetCompression, ZstdLevel};
use parquet::errors::ParquetError;
use parquet::file::properties::WriterProperties;
use platypus_core::{DriverKind, special};
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

/// How the finished file is compressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Compression {
    #[default]
    None,
    Gzip,
    Zstd,
}

impl Compression {
    /// What the compressed file adds to its name, e.g. `.gz`.
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Gzip => ".gz",
            Self::Zstd => ".zst",
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
    pub compression: Compression,
    /// A signed URL (S3, GCS) the finished file is also sent to with a `PUT`.
    pub upload_url: Option<String>,
    /// A bucket the finished file is also sent to, with the keys to sign the `PUT`.
    pub bucket: Option<s3::Bucket>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: Format::Csv,
            delimiter: ',',
            header: true,
            table: "export".to_owned(),
            driver: None,
            compression: Compression::None,
            upload_url: None,
            bucket: None,
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
    #[error("upload failed: {0}")]
    Upload(String),
}

pub type Result<T, E = ExportError> = std::result::Result<T, E>;

enum Sink {
    Parquet(Box<ArrowWriter<BufWriter<File>>>),
    Text(Box<text::TextSink>),
}

/// Whether a column of `data_type` is written as text in `format`: the text formats write
/// every value from its text, with its type to tell numbers (see [`text::TextSink`]).
fn as_text(data_type: &DataType, format: Format) -> bool {
    format == Format::Parquet && matches!(data_type, DataType::Interval(_) | DataType::Duration(_))
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
    let formatter = special::formatter(array, &FormatOptions::default())?;
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

/// Fails on a value Parquet cannot store. It keeps dates, timestamps and decimals as
/// numbers: their infinities are DuckDB's and read back as such, but a decimal `NaN` or a
/// date outside the calendar (`0000-00-00`) would come out as a wrong number.
fn parquet_values(batch: &RecordBatch) -> Result<()> {
    for (field, column) in batch.schema().fields().iter().zip(batch.columns()) {
        if special::find(column.as_ref()).is_none() {
            continue;
        }
        let found = (0..column.len())
            .filter_map(|i| special::text(column.as_ref(), i))
            .find(|text| !text.ends_with("infinity"));
        if let Some(text) = found {
            return Err(ExportError::Options(format!(
                "column `{}` holds `{text}`, which Parquet cannot store: export to another \
                 format, or cast the column to text in the query",
                field.name()
            )));
        }
    }
    Ok(())
}

pub struct Exporter {
    sink: Option<Sink>,
    schema: SchemaRef,
    format: Format,
    partial: PathBuf,
    target: PathBuf,
    rows: u64,
    compression: Compression,
    upload_url: Option<String>,
    bucket: Option<s3::Bucket>,
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
        // The error names the file asked for, not the partial one.
        let file = File::create(&partial).map_err(io_error(path))?;
        let out = BufWriter::with_capacity(1 << 20, file);
        let schema = output_schema(schema, options.format);

        let sink = match options.format {
            format @ (Format::Csv
            | Format::Json
            | Format::Ndjson
            | Format::Sql
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
                    text::Settings {
                        sql: (dialect, table),
                        delimiter,
                        header: options.header,
                    },
                )?))
            }
            Format::Parquet => {
                let properties = WriterProperties::builder()
                    .set_compression(ParquetCompression::ZSTD(ZstdLevel::try_new(3)?))
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
            compression: options.compression,
            upload_url: options.upload_url.clone(),
            bucket: options.bucket.clone(),
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
            Some(Sink::Parquet(w)) => {
                parquet_values(&batch)?;
                w.write(&batch)?;
            }
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

    /// Moves the completed file into place, compressed if asked, and uploads it.
    fn into_place(self) -> Result<u64> {
        let placed = match self.compression {
            Compression::None => {
                std::fs::rename(&self.partial, &self.target).map_err(io_error(&self.target))
            }
            c => compress(&self.partial, &self.target, c),
        };
        let _ = std::fs::remove_file(&self.partial);
        placed?;
        if let Some(url) = &self.upload_url {
            upload(&self.target, url)?;
        }
        if let Some(bucket) = &self.bucket {
            s3::put(&self.target, bucket)?;
        }
        Ok(self.rows)
    }
}

/// Writes `from` compressed to `to`, through a partial file of its own.
fn compress(from: &Path, to: &Path, compression: Compression) -> Result<()> {
    let mut partial = to.as_os_str().to_owned();
    // Not `.part`: that is `from`.
    partial.push(".compressing");
    let partial = PathBuf::from(partial);
    let write = || -> std::io::Result<()> {
        let mut input = File::open(from)?;
        let out = BufWriter::new(File::create(&partial)?);
        let out = match compression {
            Compression::Gzip => {
                let mut e = flate2::write::GzEncoder::new(out, flate2::Compression::default());
                std::io::copy(&mut input, &mut e)?;
                e.finish()?
            }
            Compression::Zstd | Compression::None => {
                let mut e = zstd::Encoder::new(out, 3)?;
                std::io::copy(&mut input, &mut e)?;
                e.finish()?
            }
        };
        out.into_inner()
            .map_err(std::io::IntoInnerError::into_error)?
            .sync_all()
    };
    let written = write().and_then(|()| std::fs::rename(&partial, to));
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written.map_err(io_error(to))
}

/// Sends the file at `path` to `url` with a `PUT`, as a signed S3 or GCS URL takes it. Errors
/// leave the URL out: its signature is a credential.
pub fn upload(path: &Path, url: &str) -> Result<()> {
    let file = File::open(path).map_err(io_error(path))?;
    // S3 takes no chunked body: the length goes first.
    let length = file.metadata().map_err(io_error(path))?.len();
    ureq::put(url)
        .header("Content-Length", length.to_string())
        .send(ureq::SendBody::from_owned_reader(file))
        .map(|_| ())
        .map_err(|e| ExportError::Upload(redact(&e.to_string(), url)))
}

/// `message` without the signature of `url`, however the HTTP client spelled the URL: the
/// query of any URL in it is cut, and each value of the query is hidden wherever it shows.
fn redact(message: &str, url: &str) -> String {
    let (base, query) = url.split_once('?').unwrap_or((url, ""));
    let mut text = message.replace(url, base);
    let values = query
        .split('&')
        .filter_map(|pair| pair.split_once('=').map(|(_, v)| v))
        .filter(|v| !v.is_empty());
    for value in values {
        text = text.replace(value, "…");
    }
    text.split_inclusive(char::is_whitespace)
        .map(|word| match word.find('?') {
            // A path or URL with a query: keep up to the `?` and the space after.
            Some(i) if word[..i].contains('/') => {
                let end = word.trim_end().len();
                format!("{}{}", &word[..i], &word[end..])
            }
            _ => word.to_owned(),
        })
        .collect()
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

        // A decimal with more digits than an Excel number keeps stays exact, as text.
        let schema = Schema::new(vec![Field::new("d", DataType::Decimal128(38, 4), true)]);
        let wide = Decimal128Array::from(vec![12_345_678_901_234_567_890_i128, -1_200])
            .with_precision_and_scale(38, 4)
            .unwrap();
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![Arc::new(wide)]).unwrap();
        let path = dir.path().join("wide.xlsx");
        let mut exporter = Exporter::create(&path, &schema, &options).unwrap();
        exporter.write(&batch).unwrap();
        exporter.finish().unwrap();
        let mut book = calamine::open_workbook_auto(&path).unwrap();
        let sheet = book.worksheet_range_at(0).unwrap().unwrap();
        let rows: Vec<_> = sheet.rows().collect();
        assert_eq!(rows[1][0], Data::String("1234567890123456.7890".into()));
        assert_eq!(rows[2][0], Data::Float(-0.12));
    }
    #[test]
    fn compresses_with_gzip_and_zstd() {
        use std::io::Read as _;
        let dir = tempfile::tempdir().unwrap();
        for compression in [Compression::Gzip, Compression::Zstd] {
            let options = ExportOptions {
                compression,
                ..ExportOptions::default()
            };
            let path = export(dir.path(), "out.csv.z", &options);
            let bytes = std::fs::read(&path).unwrap();
            let text = match compression {
                Compression::Gzip => {
                    let mut s = String::new();
                    flate2::read::GzDecoder::new(&bytes[..])
                        .read_to_string(&mut s)
                        .unwrap();
                    s
                }
                _ => String::from_utf8(zstd::decode_all(&bytes[..]).unwrap()).unwrap(),
            };
            assert!(
                text.starts_with("id,price,name,raw,at\n1,123.45,"),
                "{compression:?}: {text}"
            );
            assert!(!dir.path().join("out.csv.z.part").exists());
        }
    }

    /// A one-request HTTP server answering `status`; returns its URL and the request it got.
    fn server(status: u16) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{BufRead as _, BufReader, Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/bucket/out.csv?X-Amz-Signature=secret",
            listener.local_addr().unwrap()
        );
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut head = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let reply =
                format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            reader.get_mut().write_all(reply.as_bytes()).unwrap();
            head + &String::from_utf8(body).unwrap()
        });
        (url, handle)
    }

    #[test]
    fn uploads_to_a_signed_url() {
        let dir = tempfile::tempdir().unwrap();
        let (url, request) = server(200);
        let options = ExportOptions {
            upload_url: Some(url),
            ..ExportOptions::default()
        };
        let path = export(dir.path(), "out.csv", &options);
        let request = request.join().unwrap();
        assert!(
            request.starts_with("PUT /bucket/out.csv?X-Amz-Signature=secret"),
            "{request}"
        );
        assert!(
            request.ends_with(&std::fs::read_to_string(path).unwrap()),
            "{request}"
        );

        // A refused upload says so, without the signature.
        let (url, request) = server(403);
        std::fs::write(dir.path().join("x.csv"), "a\n").unwrap();
        let err = upload(&dir.path().join("x.csv"), &url)
            .unwrap_err()
            .to_string();
        request.join().unwrap();
        assert!(err.contains("403"), "{err}");
        assert!(!err.contains("secret"), "{err}");
    }

    #[test]
    fn redacts_signed_urls_however_spelled() {
        let url = "https://b.s3.amazonaws.com/out.csv?X-Amz-Credential=AKIA%2F2026&X-Amz-Signature=deadbeef";
        let messages = [
            format!("{url}: connection refused"),
            // Normalized by the client: another spelling of the same URL.
            "PUT https://b.s3.amazonaws.com:443/out.csv?X-Amz-Credential=AKIA%2F2026&X-Amz-Signature=deadbeef failed".to_owned(),
            "bad signature deadbeef".to_owned(),
            "redirect to /out.csv?token=other".to_owned(),
        ];
        for message in &messages {
            let shown = redact(message, url);
            for secret in ["deadbeef", "AKIA", "token=other"] {
                assert!(!shown.contains(secret), "{shown}");
            }
        }
        assert_eq!(
            redact(&messages[0], url),
            "https://b.s3.amazonaws.com/out.csv: connection refused"
        );
        assert_eq!(
            redact(&messages[1], url),
            "PUT https://b.s3.amazonaws.com:443/out.csv failed"
        );
        assert_eq!(redact("what? no", url), "what? no");
    }

    /// A date, a timestamp and a decimal column, each with a reserved value in row 0.
    fn special_batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("d", DataType::Date32, true),
            Field::new(
                "t",
                DataType::Timestamp(TimeUnit::Microsecond, Some("+00:00".into())),
                true,
            ),
            Field::new("n", DataType::Decimal128(10, 2), true),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(arrow_array::Date32Array::from(vec![
                    special::DATE_INFINITY,
                    0,
                ])),
                Arc::new(
                    TimestampMicrosecondArray::from(vec![special::TIMESTAMP_NEG_INFINITY, 0])
                        .with_timezone("+00:00"),
                ),
                Arc::new(
                    Decimal128Array::from(vec![special::DECIMAL_NAN, 150])
                        .with_precision_and_scale(10, 2)
                        .unwrap(),
                ),
            ],
        )
        .unwrap()
    }

    fn export_special(dir: &Path, name: &str, format: Format) -> Result<String> {
        let path = dir.join(name);
        let batch = special_batch();
        let options = ExportOptions {
            format,
            ..ExportOptions::default()
        };
        let mut exporter = Exporter::create(&path, &batch.schema(), &options)?;
        exporter.write(&batch)?;
        exporter.finish()?;
        Ok(std::fs::read_to_string(&path).unwrap_or_default())
    }

    #[test]
    fn exports_infinities_and_nan_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let csv = export_special(dir.path(), "s.csv", Format::Csv).unwrap();
        let lines: Vec<_> = csv.lines().collect();
        assert_eq!(lines[1], "infinity,-infinity,NaN");
        assert_eq!(lines[2], "1970-01-01,1970-01-01T00:00:00Z,1.50");
        let json = export_special(dir.path(), "s.json", Format::Json).unwrap();
        assert!(
            json.contains(r#"{"d":"infinity","t":"-infinity","n":"NaN"}"#),
            "{json}"
        );
        let sql = export_special(dir.path(), "s.sql", Format::Sql).unwrap();
        assert!(
            sql.contains("VALUES ('infinity', '-infinity', 'NaN');"),
            "{sql}"
        );
        // Parquet keeps the infinities (DuckDB's own) but has no decimal NaN.
        let err = export_special(dir.path(), "s.parquet", Format::Parquet).unwrap_err();
        assert!(err.to_string().contains("`NaN`"), "{err}");
        assert!(!dir.path().join("s.parquet").exists());
    }

    /// The file `name` of one batch of `columns`, exported as `format`.
    fn export_columns(
        dir: &Path,
        name: &str,
        format: Format,
        columns: Vec<(Field, ArrayRef)>,
    ) -> String {
        let (fields, arrays): (Vec<_>, Vec<_>) = columns.into_iter().unzip();
        let schema = Arc::new(Schema::new(fields));
        let batch = RecordBatch::try_new(Arc::clone(&schema), arrays).unwrap();
        let path = dir.join(name);
        let options = ExportOptions {
            format,
            ..ExportOptions::default()
        };
        let mut exporter = Exporter::create(&path, &schema, &options).unwrap();
        exporter.write(&batch).unwrap();
        exporter.finish().unwrap();
        std::fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn csv_tells_empty_text_from_null() {
        let dir = tempfile::tempdir().unwrap();
        let text: ArrayRef = Arc::new(StringArray::from(vec![None, Some(""), Some("x")]));
        let csv = export_columns(
            dir.path(),
            "e.csv",
            Format::Csv,
            vec![(Field::new("t", DataType::Utf8, true), text)],
        );
        assert_eq!(csv, "t\n\n\"\"\nx\n");
    }

    #[test]
    fn json_keeps_nested_values_as_json() {
        let dir = tempfile::tempdir().unwrap();
        let declared = |name: &str, t: &str| {
            Field::new(name, DataType::Utf8, true).with_metadata(std::collections::HashMap::from([
                (platypus_core::META_DB_TYPE.to_owned(), t.to_owned()),
            ]))
        };
        let json: ArrayRef = Arc::new(StringArray::from(vec![r#"{"a": 1}"#]));
        let array: ArrayRef = Arc::new(StringArray::from(vec!["{1,2}"]));
        let text = export_columns(
            dir.path(),
            "n.json",
            Format::Json,
            vec![
                (declared("j", "jsonb"), json),
                (declared("arr", "int4[]"), array),
            ],
        );
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value,
            serde_json::json!([{ "j": { "a": 1 }, "arr": [1, 2] }])
        );
    }

    #[test]
    fn excel_dates_are_dates() {
        use calamine::{Data, Reader};
        let dir = tempfile::tempdir().unwrap();
        let day: ArrayRef = Arc::new(arrow_array::Date32Array::from(vec![19_753]));
        let at: ArrayRef = Arc::new(TimestampMicrosecondArray::from(vec![1_706_697_000_000_000]));
        export_columns(
            dir.path(),
            "d.xlsx",
            Format::Xlsx,
            vec![
                (Field::new("d", DataType::Date32, true), day),
                (
                    Field::new("ts", DataType::Timestamp(TimeUnit::Microsecond, None), true),
                    at,
                ),
            ],
        );
        let mut book = calamine::open_workbook_auto(dir.path().join("d.xlsx")).unwrap();
        let sheet = book.worksheet_range_at(0).unwrap().unwrap();
        let rows: Vec<_> = sheet.rows().collect();
        assert!(matches!(rows[1][0], Data::DateTime(_)), "{:?}", rows[1][0]);
        assert!(matches!(rows[1][1], Data::DateTime(_)), "{:?}", rows[1][1]);
    }

    #[test]
    fn a_failed_export_names_the_file_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope").join("x.csv");
        let err = Exporter::create(&path, &sample().schema(), &ExportOptions::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("x.csv") && !err.contains(".part"), "{err}");
    }

    #[test]
    fn exports_cells_longer_than_excel_holds() {
        use calamine::{Data, Reader};
        let dir = tempfile::tempdir().unwrap();
        let schema = Arc::new(Schema::new(vec![Field::new("s", DataType::Utf8, true)]));
        let long = "x".repeat(40_000);
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![Arc::new(StringArray::from(vec![long.as_str(), "ok"]))],
        )
        .unwrap();
        let path = dir.path().join("long.xlsx");
        let options = ExportOptions {
            format: Format::Xlsx,
            ..ExportOptions::default()
        };
        let mut exporter = Exporter::create(&path, &schema, &options).unwrap();
        exporter.write(&batch).unwrap();
        assert_eq!(exporter.finish().unwrap(), 2);
        let mut book = calamine::open_workbook_auto(&path).unwrap();
        let sheet = book.worksheet_range_at(0).unwrap().unwrap();
        let rows: Vec<_> = sheet.rows().collect();
        let Data::String(cut) = &rows[1][0] else {
            panic!("{:?}", rows[1][0]);
        };
        assert_eq!(cut.chars().count(), 32_767);
        assert!(cut.starts_with("xxx") && cut.ends_with("Excel holds 32767]"));
        assert_eq!(rows[2][0], Data::String("ok".into()));
    }
}
