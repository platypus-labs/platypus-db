//! The formats written cell by cell from the values' text: CSV, JSON, SQL `INSERT`
//! statements, Markdown, HTML and XML tables, and Excel workbooks.

use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use arrow_array::RecordBatch;
use arrow_schema::{DataType, Schema};
use platypus_sql::Dialect;
use platypus_sql::dml::{Column, ValueKind};
use rust_xlsxwriter::{Format as CellFormat, Workbook, Worksheet, XlsxError};

use crate::import::{literal, rows_text};
use crate::json::JsonKind;
use crate::{ExportError, Format, Result, io_error};

/// Rows an Excel sheet holds, the header included.
const XLSX_MAX_ROWS: u32 = 1_048_576;

/// Integers beyond this lose digits as Excel numbers, so they are written as text.
const XLSX_MAX_EXACT: f64 = 9_007_199_254_740_992.0;

/// Significant digits an Excel number keeps: decimals with more are written as text.
const XLSX_DIGITS: usize = 15;

/// What the formats take besides the rows.
pub struct Settings {
    /// For SQL: the dialect and the quoted table name.
    pub sql: (Dialect, String),
    /// For CSV: the field separator, and whether a header row comes first.
    pub delimiter: u8,
    pub header: bool,
}

pub struct TextSink {
    format: Format,
    names: Vec<String>,
    kinds: Vec<ValueKind>,
    /// For JSON: which values are JSON already.
    json: Vec<JsonKind>,
    /// For Excel: the date or time columns, written as Excel dates.
    dates: Vec<Option<DateCell>>,
    /// For SQL in ClickHouse: the type of the lists, maps and tuples, which it reads back
    /// only as its own literals, `map('k', 1)` and `(1, 'x')`.
    nested: Vec<Option<DataType>>,
    settings: Settings,
    out: Out,
    rows: u32,
    /// Whether no row was written yet (JSON separates them with commas).
    first: bool,
}

/// A column Excel shows as a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateCell {
    Date,
    DateTime,
}

impl DateCell {
    fn of(data_type: &DataType) -> Option<Self> {
        match data_type {
            DataType::Date32 | DataType::Date64 => Some(Self::Date),
            DataType::Timestamp(..) => Some(Self::DateTime),
            _ => None,
        }
    }

    fn format(self) -> CellFormat {
        CellFormat::new().set_num_format(match self {
            Self::Date => "yyyy-mm-dd",
            Self::DateTime => "yyyy-mm-dd hh:mm:ss",
        })
    }
}

enum Out {
    File(std::io::BufWriter<std::fs::File>),
    Xlsx(Box<Workbook>),
}

impl From<XlsxError> for ExportError {
    fn from(e: XlsxError) -> Self {
        Self::Options(format!("Excel: {e}"))
    }
}

/// Characters a cell holds at most in Excel.
const XLSX_MAX_CHARS: usize = 32_767;

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Whether XML 1.0 forbids `c` in a document, even as a character reference: the control
/// characters other than tab and line breaks, and U+FFFE and U+FFFF.
fn xml_forbidden(c: char) -> bool {
    matches!(c, '\0'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}')
}

/// `bytes` in standard base64, with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0usize, |n, (i, &b)| n | (usize::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i)) & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A `<field>` element of the XML export. A value with characters XML cannot hold is
/// written in base64 (of its UTF-8), marked `encoding="base64"`.
fn xml_field(s: &mut String, name: &str, value: Option<&str>) {
    let name: String = name
        .chars()
        .map(|c| if xml_forbidden(c) { '\u{fffd}' } else { c })
        .collect();
    let name = xml_escape(&name);
    let _ = match value {
        None => writeln!(s, "    <field name=\"{name}\" null=\"true\"/>"),
        Some(v) if v.chars().any(xml_forbidden) => writeln!(
            s,
            "    <field name=\"{name}\" encoding=\"base64\">{}</field>",
            base64(v.as_bytes())
        ),
        Some(v) => writeln!(s, "    <field name=\"{name}\">{}</field>", xml_escape(v)),
    };
}

/// `v` cut to fit an Excel cell, ending with a note of how long it was.
fn xlsx_text(v: &str) -> std::borrow::Cow<'_, str> {
    let Some((cut, _)) = v.char_indices().nth(XLSX_MAX_CHARS) else {
        return v.into();
    };
    let total = v.chars().count();
    let note = format!("… [cut: {total} characters, Excel holds {XLSX_MAX_CHARS}]");
    let keep = XLSX_MAX_CHARS - note.chars().count();
    let end = v.char_indices().nth(keep).map_or(cut, |(i, _)| i);
    format!("{}{note}", &v[..end]).into()
}

/// The ClickHouse literal of the JSON text `json` of a list, map or tuple of `data_type`:
/// `[1, 2]`, `map('k', 1)`, `(1, 'x')`. `None` for text that is not that JSON.
fn clickhouse_nested(data_type: &DataType, json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    nested_literal(data_type, &value)
}

fn nested_literal(data_type: &DataType, value: &serde_json::Value) -> Option<String> {
    use serde_json::Value;
    let quote = |s: &str| Dialect::ClickHouse.quote_literal(s);
    Some(match (data_type, value) {
        (_, Value::Null) => "NULL".to_owned(),
        (DataType::List(item) | DataType::LargeList(item), Value::Array(items)) => {
            let items = items
                .iter()
                .map(|v| nested_literal(item.data_type(), v))
                .collect::<Option<Vec<_>>>()?;
            format!("[{}]", items.join(", "))
        }
        (DataType::Struct(fields), Value::Object(object)) => {
            let items = fields
                .iter()
                .map(|f| nested_literal(f.data_type(), object.get(f.name())?))
                .collect::<Option<Vec<_>>>()?;
            format!("({})", items.join(", "))
        }
        (DataType::Map(entries, _), Value::Object(object)) => {
            let DataType::Struct(kv) = entries.data_type() else {
                return None;
            };
            let [key, item] = kv.iter().collect::<Vec<_>>()[..] else {
                return None;
            };
            let mut args = Vec::with_capacity(object.len() * 2);
            for (k, v) in object {
                let number = k.parse::<f64>().is_ok() && key.data_type().is_numeric();
                args.push(if number { k.clone() } else { quote(k) });
                args.push(nested_literal(item.data_type(), v)?);
            }
            format!("map({})", args.join(", "))
        }
        (_, Value::String(s)) => quote(s),
        (_, Value::Number(n)) => n.to_string(),
        (_, Value::Bool(b)) => b.to_string(),
        _ => return None,
    })
}

fn markdown_cell(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace('\n', "<br>")
}

impl TextSink {
    pub fn new(
        format: Format,
        schema: &Schema,
        out: std::io::BufWriter<std::fs::File>,
        settings: Settings,
    ) -> Result<Self> {
        let names: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
        let kinds = schema
            .fields()
            .iter()
            .map(|f| ValueKind::of(f.data_type()))
            .collect();
        let json = schema.fields().iter().map(|f| JsonKind::of(f)).collect();
        let dates = schema
            .fields()
            .iter()
            .map(|f| DateCell::of(f.data_type()))
            .collect();
        let nested = schema
            .fields()
            .iter()
            .map(|f| {
                (format == Format::Sql
                    && settings.sql.0 == Dialect::ClickHouse
                    && matches!(
                        f.data_type(),
                        DataType::List(_) | DataType::Struct(_) | DataType::Map(..)
                    ))
                .then(|| f.data_type().clone())
            })
            .collect();
        let out = if format == Format::Xlsx {
            let mut workbook = Workbook::new();
            let sheet = workbook.add_worksheet_with_constant_memory()?;
            for (c, name) in (0u16..).zip(&names) {
                sheet.write_string(0, c, name)?;
            }
            // The file the workbook saves to replaces this one at the end.
            drop(out);
            Out::Xlsx(Box::new(workbook))
        } else {
            Out::File(out)
        };
        let mut sink = Self {
            format,
            names,
            kinds,
            json,
            dates,
            nested,
            settings,
            out,
            rows: 0,
            first: true,
        };
        let head = sink.head();
        sink.put(&head)?;
        Ok(sink)
    }

    fn put(&mut self, text: &str) -> Result<()> {
        if let Out::File(out) = &mut self.out {
            out.write_all(text.as_bytes())
                .map_err(io_error(Path::new("export")))?;
        }
        Ok(())
    }

    fn head(&self) -> String {
        match self.format {
            Format::Csv if self.settings.header => {
                let mut s = String::new();
                csv_line(&mut s, self.settings.delimiter, self.names.iter().map(Some));
                s
            }
            Format::Json => "[".to_owned(),
            Format::Markdown => {
                let names: Vec<String> = self.names.iter().map(|n| markdown_cell(n)).collect();
                format!(
                    "| {} |\n|{}|\n",
                    names.join(" | "),
                    vec![" --- "; names.len()].join("|")
                )
            }
            Format::Html => {
                let mut s = String::from(
                    "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\"></head>\n<body>\n<table>\n<thead><tr>",
                );
                for n in &self.names {
                    let _ = write!(s, "<th>{}</th>", xml_escape(n));
                }
                s.push_str("</tr></thead>\n<tbody>\n");
                s
            }
            Format::Xml => "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rows>\n".to_owned(),
            _ => String::new(),
        }
    }

    fn tail(&self) -> &'static str {
        match self.format {
            Format::Json => "]\n",
            Format::Html => "</tbody>\n</table>\n</body>\n</html>\n",
            Format::Xml => "</rows>\n",
            _ => "",
        }
    }

    pub fn write(&mut self, batch: &RecordBatch) -> Result<()> {
        let rows = rows_text(batch)?;
        if let Out::Xlsx(workbook) = &mut self.out {
            let sheet = workbook.worksheet_from_index(0)?;
            let formats = self.dates.iter().map(|d| d.map(DateCell::format)).collect();
            let columns = XlsxColumns {
                kinds: &self.kinds,
                dates: formats,
            };
            for row in &rows {
                self.rows += 1;
                if self.rows >= XLSX_MAX_ROWS {
                    return Err(ExportError::Options(format!(
                        "an Excel sheet holds at most {} rows",
                        XLSX_MAX_ROWS - 1
                    )));
                }
                xlsx_row(sheet, self.rows, &columns, row)?;
            }
            return Ok(());
        }
        let mut text = String::new();
        for row in &rows {
            self.row(&mut text, row)?;
        }
        self.put(&text)
    }

    fn row(&mut self, s: &mut String, row: &[Option<String>]) -> Result<()> {
        match self.format {
            Format::Csv => {
                csv_line(s, self.settings.delimiter, row.iter().map(Option::as_ref));
            }
            Format::Json | Format::Ndjson => {
                if self.format == Format::Json && !self.first {
                    s.push_str(",\n");
                }
                self.first = false;
                s.push('{');
                for (i, ((name, json), v)) in self.names.iter().zip(&self.json).zip(row).enumerate()
                {
                    if i > 0 {
                        s.push(',');
                    }
                    crate::json::string(s, name);
                    s.push(':');
                    json.value(s, self.kinds[i], v.as_deref());
                }
                s.push('}');
                if self.format == Format::Ndjson {
                    s.push('\n');
                }
            }
            Format::Markdown => {
                let cells: Vec<String> = row
                    .iter()
                    .map(|v| v.as_deref().map(markdown_cell).unwrap_or_default())
                    .collect();
                let _ = writeln!(s, "| {} |", cells.join(" | "));
            }
            Format::Html => {
                s.push_str("<tr>");
                for v in row {
                    let _ = write!(
                        s,
                        "<td>{}</td>",
                        v.as_deref().map(xml_escape).unwrap_or_default()
                    );
                }
                s.push_str("</tr>\n");
            }
            Format::Xml => {
                s.push_str("  <row>\n");
                for (name, v) in self.names.iter().zip(row) {
                    xml_field(s, name, v.as_deref());
                }
                s.push_str("  </row>\n");
            }
            Format::Sql => {
                let (dialect, table) = &self.settings.sql;
                let mut literals = Vec::with_capacity(row.len());
                for (i, ((name, kind), v)) in
                    self.names.iter().zip(&self.kinds).zip(row).enumerate()
                {
                    let nested = self.nested[i]
                        .as_ref()
                        .zip(v.as_deref())
                        .and_then(|(t, json)| clickhouse_nested(t, json));
                    if let Some(lit) = nested {
                        literals.push(lit);
                        continue;
                    }
                    let column = Column {
                        name,
                        kind: *kind,
                        default: None,
                    };
                    literals.push(
                        literal(&column, *dialect, v.as_deref())
                            .map_err(|e| ExportError::Options(e.to_string()))?,
                    );
                }
                let columns: Vec<_> = self.names.iter().map(|n| dialect.quote_ident(n)).collect();
                let _ = writeln!(
                    s,
                    "INSERT INTO {table} ({}) VALUES ({});",
                    columns.join(", "),
                    literals.join(", ")
                );
            }
            Format::Parquet | Format::Xlsx => {}
        }
        Ok(())
    }

    /// Completes the file at `partial`.
    pub fn finish(mut self, partial: &Path) -> Result<()> {
        let tail = self.tail();
        self.put(tail)?;
        match self.out {
            Out::File(out) => {
                let file = out
                    .into_inner()
                    .map_err(|e| io_error(partial)(e.into_error()))?;
                file.sync_all().map_err(io_error(partial))
            }
            Out::Xlsx(mut workbook) => Ok(workbook.save(partial)?),
        }
    }
}

/// A CSV line of `values`: a value is quoted when it holds the delimiter, a quote or a line
/// break, and an empty text is `""`, so that it differs from NULL, which is nothing.
fn csv_line<'a>(s: &mut String, delimiter: u8, values: impl Iterator<Item = Option<&'a String>>) {
    let delimiter = char::from(delimiter);
    for (i, v) in values.enumerate() {
        if i > 0 {
            s.push(delimiter);
        }
        let Some(v) = v else { continue };
        if v.is_empty() || v.contains([delimiter, '"', '\n', '\r']) {
            s.push('"');
            s.push_str(&v.replace('"', "\"\""));
            s.push('"');
        } else {
            s.push_str(v);
        }
    }
    s.push('\n');
}

/// How the columns of a sheet are written.
struct XlsxColumns<'a> {
    kinds: &'a [ValueKind],
    /// The cell format of a date or time column.
    dates: Vec<Option<CellFormat>>,
}

/// One row of a sheet: numbers, booleans and dates as such, the rest as text.
fn xlsx_row(
    sheet: &mut Worksheet,
    r: u32,
    columns: &XlsxColumns<'_>,
    row: &[Option<String>],
) -> Result<()> {
    for (((c, kind), date), v) in (0u16..).zip(columns.kinds).zip(&columns.dates).zip(row) {
        let Some(v) = v else { continue };
        if let Some(format) = date
            && let Some(serial) = excel_serial(v)
        {
            sheet.write_number_with_format(r, c, serial, format)?;
            continue;
        }
        let number = match kind {
            ValueKind::Decimal if significant_digits(v) > XLSX_DIGITS => None,
            ValueKind::Integer | ValueKind::Float | ValueKind::Real | ValueKind::Decimal => v
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && n.abs() < XLSX_MAX_EXACT),
            _ => None,
        };
        match (kind, number) {
            (_, Some(n)) => sheet.write_number(r, c, n)?,
            (ValueKind::Bool, _) => sheet.write_boolean(r, c, v == "true")?,
            _ => sheet.write_string(r, c, xlsx_text(v))?,
        };
    }
    Ok(())
}

/// Excel's serial number of a date or time as the export writes it (`2024-01-31`,
/// `2024-01-31T10:30:00`, with a fraction or an offset, taken in UTC): days since 1899-12-30.
/// `None` for one Excel cannot show, before 1900 or not a date.
fn excel_serial(text: &str) -> Option<f64> {
    use chrono::{NaiveDate, NaiveDateTime};
    let at = if let Ok(day) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        day.and_hms_opt(0, 0, 0)?
    } else if let Ok(zoned) = chrono::DateTime::parse_from_rfc3339(text) {
        zoned.naive_utc()
    } else {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").ok()?
    };
    let epoch = NaiveDate::from_ymd_opt(1899, 12, 30)?.and_hms_opt(0, 0, 0)?;
    if at < NaiveDate::from_ymd_opt(1900, 3, 1)?.and_hms_opt(0, 0, 0)? {
        return None;
    }
    let micros = (at - epoch).num_microseconds()?;
    #[expect(clippy::cast_precision_loss, reason = "microseconds of Excel's years")]
    Some(micros as f64 / 86_400_000_000.0)
}

/// The significant digits of a decimal's text, e.g. 3 for `-0.0120`.
fn significant_digits(v: &str) -> usize {
    let v = if v.contains('.') {
        v.trim_end_matches('0')
    } else {
        v
    };
    v.chars()
        .filter(char::is_ascii_digit)
        .skip_while(|&c| c == '0')
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_base64() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("\u{1}x", "AXg="),
        ];
        for (input, output) in cases {
            assert_eq!(base64(input.as_bytes()), output, "{input:?}");
        }
    }

    #[test]
    fn writes_xml_control_characters_in_base64() {
        let mut s = String::new();
        xml_field(&mut s, "a\u{2}b", Some("x\u{1}y"));
        xml_field(&mut s, "n", Some("tab\tand <tag>"));
        xml_field(&mut s, "z", None);
        assert_eq!(
            s,
            "    <field name=\"a\u{fffd}b\" encoding=\"base64\">eAF5</field>\n    \
             <field name=\"n\">tab\tand &lt;tag&gt;</field>\n    \
             <field name=\"z\" null=\"true\"/>\n"
        );
    }

    #[test]
    fn clickhouse_reads_maps_and_tuples_as_functions_and_tuples() {
        use arrow_schema::Field;
        use std::sync::Arc;
        let entries = Field::new(
            "entries",
            DataType::Struct(
                vec![
                    Field::new("key", DataType::Utf8, false),
                    Field::new("value", DataType::Int32, true),
                ]
                .into(),
            ),
            false,
        );
        let map = DataType::Map(Arc::new(entries), false);
        assert_eq!(
            clickhouse_nested(&map, r#"{"k":1,"it's":null}"#).as_deref(),
            Some("map('k', 1, 'it''s', NULL)")
        );
        assert_eq!(clickhouse_nested(&map, "{}").as_deref(), Some("map()"));
        let tuple = DataType::Struct(
            vec![
                Field::new("1", DataType::Int32, true),
                Field::new("2", DataType::Utf8, true),
            ]
            .into(),
        );
        assert_eq!(
            clickhouse_nested(&tuple, r#"{"1":1,"2":"x"}"#).as_deref(),
            Some("(1, 'x')")
        );
        let list = DataType::List(Arc::new(Field::new("item", tuple, true)));
        assert_eq!(
            clickhouse_nested(&list, r#"[{"1":1,"2":"x"}]"#).as_deref(),
            Some("[(1, 'x')]")
        );
        assert_eq!(clickhouse_nested(&map, "nope"), None);
    }

    #[test]
    fn cuts_text_longer_than_an_excel_cell() {
        assert_eq!(xlsx_text("short"), "short");
        // Two bytes each: the limit counts characters.
        let exact = "\u{e9}".repeat(XLSX_MAX_CHARS);
        assert_eq!(xlsx_text(&exact), exact.as_str());
        let long = "\u{e9}".repeat(40_000);
        let cut = xlsx_text(&long);
        assert_eq!(cut.chars().count(), XLSX_MAX_CHARS);
        assert!(cut.ends_with("[cut: 40000 characters, Excel holds 32767]"));
    }
}
