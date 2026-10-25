//! Data files opened as a database: CSV, TSV, JSON, NDJSON and Parquet become a view named
//! after the file, which DuckDB reads again at each query; each sheet of an XLSX workbook
//! becomes a table, loaded once with `calamine` since DuckDB reads Excel only with an
//! extension it would have to download.

use std::path::Path;

use calamine::{Data, Reader};
use duckdb::types::{TimeUnit, Value};
use platypus_core::{Error, Result};
use platypus_sql::Dialect;

/// The DuckDB function that reads a file with this extension (lower case).
fn reader(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "csv" | "tsv" => "read_csv",
        "json" | "ndjson" | "jsonl" => "read_json",
        "parquet" => "read_parquet",
        _ => return None,
    })
}

fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn is_workbook(extension: &str) -> bool {
    matches!(extension, "xlsx" | "xlsm" | "xls" | "ods")
}

/// Whether `path` names a data file rather than a DuckDB database.
#[must_use]
pub fn is_data_file(path: &str) -> bool {
    let extension = extension(path);
    reader(&extension).is_some() || is_workbook(&extension)
}

/// Creates in `conn`, an empty in-memory database, the view or the tables of the file.
pub fn load(conn: &duckdb::Connection, path: &str) -> Result<()> {
    let extension = extension(path);
    if is_workbook(&extension) {
        return load_workbook(conn, path);
    }
    let reader =
        reader(&extension).ok_or_else(|| Error::Config(format!("{path} is not a data file")))?;
    let name = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("data");
    let d = Dialect::DuckDb;
    // The file is read again at each query: a missing or unreadable file fails here first.
    let sql = format!(
        "CREATE OR REPLACE VIEW {} AS SELECT * FROM {reader}({}); SELECT * FROM {0} LIMIT 0",
        d.quote_ident(name),
        d.quote_literal(path),
    );
    conn.execute_batch(&sql)
        .map_err(|e| Error::Connect(format!("cannot read {path}: {e}")))
}

/// The DuckDB type of a column of cells: the narrowest one every non-empty cell fits.
fn column_type<'a>(cells: impl Iterator<Item = &'a Data>) -> &'static str {
    let mut kind = None;
    for cell in cells {
        let this = match cell {
            Data::Empty => continue,
            Data::Int(_) => "BIGINT",
            Data::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => "BIGINT",
            Data::Float(_) => "DOUBLE",
            Data::Bool(_) => "BOOLEAN",
            Data::DateTime(d) if d.is_datetime() => "TIMESTAMP",
            _ => "VARCHAR",
        };
        kind = match (kind, this) {
            (None, t) => Some(t),
            (Some(a), b) if a == b => Some(a),
            (Some("BIGINT" | "DOUBLE"), "BIGINT" | "DOUBLE") => Some("DOUBLE"),
            _ => return "VARCHAR",
        };
    }
    kind.unwrap_or("VARCHAR")
}

/// Days from 1899-12-30, Excel's day zero, to 1970-01-01.
const EXCEL_EPOCH_DAYS: f64 = 25569.0;

#[expect(
    clippy::cast_possible_truncation,
    reason = "Excel dates are far within i64 microseconds"
)]
fn cell_value(cell: &Data, column_type: &str) -> Value {
    match (cell, column_type) {
        (Data::Empty, _) => Value::Null,
        (Data::Int(i), "BIGINT" | "DOUBLE") => Value::BigInt(*i),
        (Data::Float(f), "BIGINT") => Value::BigInt(*f as i64),
        (Data::Float(f), "DOUBLE") => Value::Double(*f),
        (Data::Bool(b), "BOOLEAN") => Value::Boolean(*b),
        (Data::DateTime(d), "TIMESTAMP") => Value::Timestamp(
            TimeUnit::Microsecond,
            ((d.as_f64() - EXCEL_EPOCH_DAYS) * 86_400_000_000.0).round() as i64,
        ),
        (cell, _) => Value::Text(cell.to_string()),
    }
}

/// One table per sheet; the first row holds the column names.
fn load_workbook(conn: &duckdb::Connection, path: &str) -> Result<()> {
    let fail = |e: &dyn std::fmt::Display| Error::Connect(format!("cannot read {path}: {e}"));
    let mut workbook = calamine::open_workbook_auto(path).map_err(|e| fail(&e))?;
    let d = Dialect::DuckDb;
    for sheet in workbook.sheet_names() {
        let range = workbook.worksheet_range(&sheet).map_err(|e| fail(&e))?;
        let mut rows = range.rows();
        let Some(header) = rows.next() else { continue };
        let names = column_names(header);
        let types: Vec<&str> = (0..names.len())
            .map(|i| column_type(range.rows().skip(1).filter_map(|r| r.get(i))))
            .collect();
        let columns: Vec<String> = names
            .iter()
            .zip(&types)
            .map(|(n, t)| format!("{} {t}", d.quote_ident(n)))
            .collect();
        let table = d.quote_ident(&sheet);
        conn.execute_batch(&format!(
            "CREATE OR REPLACE TABLE {table} ({})",
            columns.join(", ")
        ))
        .map_err(|e| fail(&e))?;
        let mut appender = conn.appender(&sheet).map_err(|e| fail(&e))?;
        for row in rows {
            let values = types
                .iter()
                .enumerate()
                .map(|(i, t)| row.get(i).map_or(Value::Null, |c| cell_value(c, t)));
            appender
                .append_row(duckdb::appender_params_from_iter(values))
                .map_err(|e| fail(&e))?;
        }
        appender.flush().map_err(|e| fail(&e))?;
    }
    Ok(())
}

/// The header cells as column names: blanks become `column<n>`, repeats get a suffix.
fn column_names(header: &[Data]) -> Vec<String> {
    let mut names: Vec<String> = Vec::with_capacity(header.len());
    for (i, cell) in header.iter().enumerate() {
        let text = cell.to_string();
        let base = if text.trim().is_empty() {
            format!("column{}", i + 1)
        } else {
            text.trim().to_owned()
        };
        let mut name = base.clone();
        let mut n = 1;
        while names.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
            n += 1;
            name = format!("{base}_{n}");
        }
        names.push(name);
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_files_by_extension() {
        assert!(is_data_file("C:/x/Sales.CSV"));
        assert!(is_data_file("a.parquet"));
        assert!(is_data_file("a.ndjson"));
        assert!(is_data_file("a.xlsx"));
        assert!(!is_data_file("a.duckdb"));
        assert!(!is_data_file(":memory:"));
    }

    #[test]
    fn sheet_column_types() {
        let t = |cells: &[Data]| column_type(cells.iter());
        assert_eq!(t(&[Data::Float(1.0), Data::Empty, Data::Int(3)]), "BIGINT");
        assert_eq!(t(&[Data::Float(1.5), Data::Int(3)]), "DOUBLE");
        assert_eq!(t(&[Data::Bool(true)]), "BOOLEAN");
        assert_eq!(t(&[Data::Int(1), Data::String("x".into())]), "VARCHAR");
        assert_eq!(t(&[Data::Empty]), "VARCHAR");
    }

    #[test]
    fn header_names() {
        let header = [
            Data::String("id".into()),
            Data::Empty,
            Data::String("ID".into()),
        ];
        assert_eq!(column_names(&header), ["id", "column2", "ID_2"]);
    }
}
