//! The formats written cell by cell from the values' text: SQL `INSERT` statements,
//! Markdown, HTML and XML tables, and Excel workbooks.

use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use arrow_array::RecordBatch;
use arrow_schema::Schema;
use platypus_sql::Dialect;
use platypus_sql::dml::{Column, ValueKind};
use rust_xlsxwriter::{Workbook, Worksheet, XlsxError};

use crate::import::rows_text;
use crate::{ExportError, Format, Result, io_error};

/// Rows an Excel sheet holds, the header included.
const XLSX_MAX_ROWS: u32 = 1_048_576;

/// Integers beyond this lose digits as Excel numbers, so they are written as text.
const XLSX_MAX_EXACT: f64 = 9_007_199_254_740_992.0;

/// Significant digits an Excel number keeps: decimals with more are written as text.
const XLSX_DIGITS: usize = 15;

pub struct TextSink {
    format: Format,
    names: Vec<String>,
    kinds: Vec<ValueKind>,
    /// For SQL: the dialect and the quoted table name.
    sql: (Dialect, String),
    out: Out,
    rows: u32,
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

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
        sql: (Dialect, String),
    ) -> Result<Self> {
        let names: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
        let kinds = schema
            .fields()
            .iter()
            .map(|f| ValueKind::of(f.data_type()))
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
            sql,
            out,
            rows: 0,
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
            Format::Html => "</tbody>\n</table>\n</body>\n</html>\n",
            Format::Xml => "</rows>\n",
            _ => "",
        }
    }

    pub fn write(&mut self, batch: &RecordBatch) -> Result<()> {
        let rows = rows_text(batch)?;
        if let Out::Xlsx(workbook) = &mut self.out {
            let sheet = workbook.worksheet_from_index(0)?;
            for row in &rows {
                self.rows += 1;
                if self.rows >= XLSX_MAX_ROWS {
                    return Err(ExportError::Options(format!(
                        "an Excel sheet holds at most {} rows",
                        XLSX_MAX_ROWS - 1
                    )));
                }
                xlsx_row(sheet, self.rows, &self.kinds, row)?;
            }
            return Ok(());
        }
        let mut text = String::new();
        for row in &rows {
            self.row(&mut text, row)?;
        }
        self.put(&text)
    }

    fn row(&self, s: &mut String, row: &[Option<String>]) -> Result<()> {
        match self.format {
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
                    let name = xml_escape(name);
                    match v {
                        Some(v) => {
                            let _ =
                                writeln!(s, "    <field name=\"{name}\">{}</field>", xml_escape(v));
                        }
                        None => {
                            let _ = writeln!(s, "    <field name=\"{name}\" null=\"true\"/>");
                        }
                    }
                }
                s.push_str("  </row>\n");
            }
            Format::Sql => {
                let (dialect, table) = &self.sql;
                let mut literals = Vec::with_capacity(row.len());
                for ((name, kind), v) in self.names.iter().zip(&self.kinds).zip(row) {
                    let column = Column {
                        name,
                        kind: *kind,
                        default: None,
                    };
                    literals.push(
                        column
                            .literal(*dialect, v.as_deref())
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
            _ => {}
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

/// One row of a sheet: numbers and booleans as such, the rest as text.
fn xlsx_row(
    sheet: &mut Worksheet,
    r: u32,
    kinds: &[ValueKind],
    row: &[Option<String>],
) -> Result<()> {
    for ((c, kind), v) in (0u16..).zip(kinds).zip(row) {
        let Some(v) = v else { continue };
        let number = match kind {
            ValueKind::Decimal if significant_digits(v) > XLSX_DIGITS => None,
            ValueKind::Integer | ValueKind::Float | ValueKind::Decimal => v
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && n.abs() < XLSX_MAX_EXACT),
            _ => None,
        };
        match (kind, number) {
            (_, Some(n)) => sheet.write_number(r, c, n)?,
            (ValueKind::Bool, _) => sheet.write_boolean(r, c, v == "true")?,
            _ => sheet.write_string(r, c, v)?,
        };
    }
    Ok(())
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
