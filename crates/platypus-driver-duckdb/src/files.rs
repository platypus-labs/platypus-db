//! Data files opened as a database: CSV, TSV, JSON, NDJSON and Parquet become a view named
//! after the file, which DuckDB reads again at each query; each sheet of an XLSX workbook
//! becomes a table, loaded once with `calamine` since DuckDB reads Excel only with an
//! extension it would have to download.
//!
//! A CSV or TSV file is checked first: its lines must all hold as many values, or the load
//! fails naming those that do not, unless the reader is told to leave them out. A file that
//! is not UTF-8 (its encoding told from the bytes, or given) is converted and loaded as a
//! table. Quoted empty values stay empty texts, and decimals with more digits than a double
//! keeps become DECIMAL.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use calamine::{Data, Reader};
use duckdb::types::{TimeUnit, Value};
use encoding_rs::Encoding;
use platypus_core::{Error, Result};
use platypus_sql::Dialect;

use crate::csvfile;

/// How a delimited text file is read, from a connection's `options`: what the import dialog
/// chose.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ReadOptions {
    /// The text encoding by its label (`windows-1252`, `utf-16le`...); `None` tells it from
    /// the bytes: a byte order mark, else UTF-8 when the bytes are, else Windows-1252.
    pub encoding: Option<String>,
    /// Lines with another number of values than the file's are left out, instead of failing.
    pub skip_bad_lines: bool,
}

impl ReadOptions {
    /// The option of [`Self::encoding`].
    pub const ENCODING: &'static str = "encoding";
    /// The option of [`Self::skip_bad_lines`], `true` to set it.
    pub const SKIP_BAD_LINES: &'static str = "skip_bad_lines";

    #[must_use]
    pub fn of(options: &BTreeMap<String, String>) -> Self {
        Self {
            encoding: options
                .get(Self::ENCODING)
                .map(|e| e.trim().to_owned())
                .filter(|e| !e.is_empty() && !e.eq_ignore_ascii_case("auto")),
            skip_bad_lines: options
                .get(Self::SKIP_BAD_LINES)
                .is_some_and(|v| v.eq_ignore_ascii_case("true")),
        }
    }
}

pub use crate::csvfile::BAD_LINES;

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
pub fn load(conn: &duckdb::Connection, path: &str, options: &ReadOptions) -> Result<()> {
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
    if reader == "read_csv" {
        let tab = extension == "tsv";
        return load_csv(conn, path, name, tab, options)
            .map_err(|e| Error::Connect(format!("cannot read {path}: {e}")));
    }
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

/// Names the files [`load_csv`] writes, apart within the process.
static COPIES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Loads the CSV (`tab`: TSV) file at `path` as `name`: a view on the file when it can be
/// read as it is, else a table of its UTF-8 copy without the lines left out.
fn load_csv(
    conn: &duckdb::Connection,
    path: &str,
    name: &str,
    tab: bool,
    options: &ReadOptions,
) -> std::result::Result<(), String> {
    let chosen = match &options.encoding {
        Some(label) => Some(
            Encoding::for_label(label.as_bytes())
                .ok_or_else(|| format!("{label} is not an encoding"))?,
        ),
        None => None,
    };
    let mut sample = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| {
            f.take(csvfile::SAMPLE_BYTES as u64)
                .read_to_end(&mut sample)
        })
        .map_err(|e| e.to_string())?;
    let delimiter = |sample: &[u8]| {
        if tab {
            b'\t'
        } else {
            csvfile::delimiter(sample, sample.len() < csvfile::SAMPLE_BYTES)
        }
    };
    // Another encoding than UTF-8, chosen or named by a byte order mark: the whole file is
    // converted first, as its bytes may not even split into lines (UTF-16).
    let named = chosen.or_else(|| Encoding::for_bom(&sample).map(|(e, _)| e));
    let (text, delimiter, mut scan) = if let Some(encoding) =
        named.filter(|e| *e != encoding_rs::UTF_8)
    {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let text = csvfile::decode(&bytes, encoding)?;
        let d = delimiter(&text[..text.len().min(csvfile::SAMPLE_BYTES)]);
        let scan = csvfile::scan(&text[..], d).map_err(|e| e.to_string())?;
        (Some(text), d, scan)
    } else {
        let d = delimiter(&sample);
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let scan = csvfile::scan(std::io::BufReader::new(file), d).map_err(|e| e.to_string())?;
        let text = match (scan.utf8, chosen) {
            (true, _) => None,
            (false, Some(_)) => return Err("the file is not UTF-8 text".to_owned()),
            // Bytes that are no UTF-8: Windows-1252, as spreadsheets save CSV on Windows.
            (false, None) => {
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                Some(csvfile::decode(&bytes, encoding_rs::WINDOWS_1252)?)
            }
        };
        (text, d, scan)
    };
    if !scan.bad.is_empty() && !options.skip_bad_lines {
        let counted = match &text {
            Some(text) => scan.count_lines(&text[..]),
            None => std::fs::File::open(path).and_then(|f| scan.count_lines(f)),
        };
        counted.map_err(|e| e.to_string())?;
        return Err(scan.bad_lines_error());
    }
    let skip: Vec<u64> = scan.bad.iter().map(|b| b.record).collect();
    let copy = if text.is_some() || !skip.is_empty() {
        let text = match text {
            Some(text) => text,
            None => std::fs::read(path).map_err(|e| e.to_string())?,
        };
        let kept = csvfile::without(&text, delimiter, &skip).map_err(|e| e.to_string())?;
        let n = COPIES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let copy =
            std::env::temp_dir().join(format!("platypus-import-{}-{n}.csv", std::process::id()));
        std::fs::write(&copy, kept).map_err(|e| e.to_string())?;
        Some(copy)
    } else {
        None
    };
    let source = copy.as_ref().map_or(path, |c| c.to_str().unwrap_or(path));
    let loaded = create_csv(conn, name, source, delimiter, copy.is_some());
    if let Some(copy) = &copy {
        let _ = std::fs::remove_file(copy);
    }
    loaded
}

/// Creates `name` from the CSV file at `source`: a view, or a `table` loaded at once. Quoted
/// empty values are empty texts, and decimals beyond a double's digits stay exact.
fn create_csv(
    conn: &duckdb::Connection,
    name: &str,
    source: &str,
    delimiter: u8,
    table: bool,
) -> std::result::Result<(), String> {
    let d = Dialect::DuckDb;
    let read = |extra: &str| {
        format!(
            "read_csv({}, delim = {}, quote = '\"', escape = '\"', skip = 0, \
             allow_quoted_nulls = false{extra})",
            d.quote_literal(source),
            d.quote_literal(&char::from(delimiter).to_string())
        )
    };
    let types =
        exact_types(conn, &read(""), &read(", all_varchar = true")).map_err(|e| e.to_string())?;
    let name = d.quote_ident(name);
    let sql = if table {
        format!(
            "CREATE OR REPLACE TABLE {name} AS SELECT * FROM {}",
            read(&types)
        )
    } else {
        format!(
            "CREATE OR REPLACE VIEW {name} AS SELECT * FROM {}; SELECT * FROM {name} LIMIT 0",
            read(&types)
        )
    };
    conn.execute_batch(&sql).map_err(|e| e.to_string())
}

/// Significant digits a double keeps.
const DOUBLE_DIGITS: i64 = 15;

/// `, types = {…}` for the columns `read` (a `read_csv` call) takes as DOUBLE whose values
/// have more digits than a double keeps: a DECIMAL that holds them all, or VARCHAR beyond 38
/// digits. `text` reads the same file as text. Empty when there are none.
fn exact_types(conn: &duckdb::Connection, read: &str, text: &str) -> duckdb::Result<String> {
    let d = Dialect::DuckDb;
    let mut describe = conn.prepare(&format!("DESCRIBE SELECT * FROM {read}"))?;
    let doubles: Vec<String> = describe
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(std::result::Result::ok)
        .filter(|(_, t)| t == "DOUBLE")
        .map(|(name, _)| name)
        .collect();
    if doubles.is_empty() {
        return Ok(String::new());
    }
    let measures: Vec<String> = doubles
        .iter()
        .map(|name| {
            let c = d.quote_ident(name);
            format!(
                "max(length(regexp_extract({c}, '^\\s*[+-]?0*([0-9]*)', 1))), \
                 max(length(regexp_extract({c}, '\\.([0-9]*)\\s*$', 1))), \
                 bool_or(NOT regexp_full_match(trim({c}), '[+-]?[0-9]*\\.?[0-9]*'))"
            )
        })
        .collect();
    let sql = format!("SELECT {} FROM {text}", measures.join(", "));
    let types = conn.query_row(&sql, [], |row| {
        let mut types = Vec::new();
        for (i, name) in doubles.iter().enumerate() {
            let whole = row.get::<_, Option<i64>>(3 * i)?.unwrap_or(0);
            let fraction = row.get::<_, Option<i64>>(3 * i + 1)?.unwrap_or(0);
            let other = row.get::<_, Option<bool>>(3 * i + 2)?.unwrap_or(false);
            let digits = whole + fraction;
            if other || digits <= DOUBLE_DIGITS {
                continue;
            }
            let exact = if digits <= 38 {
                format!("DECIMAL({digits}, {fraction})")
            } else {
                "VARCHAR".to_owned()
            };
            types.push(format!(
                "{}: {}",
                d.quote_literal(name),
                d.quote_literal(&exact)
            ));
        }
        Ok(types)
    })?;
    Ok(if types.is_empty() {
        String::new()
    } else {
        format!(", types = {{{}}}", types.join(", "))
    })
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
        // A date among texts reads as Excel shows it, not as its serial number.
        (Data::DateTime(d), _) if d.is_datetime() => Value::Text(excel_date_text(d.as_f64())),
        (cell, _) => Value::Text(cell.to_string()),
    }
}

/// Excel's serial day number `serial` as `YYYY-MM-DD`, with ` HH:MM:SS` when it has a time.
#[expect(
    clippy::cast_possible_truncation,
    reason = "Excel dates are far within i64 seconds"
)]
fn excel_date_text(serial: f64) -> String {
    let seconds = ((serial - EXCEL_EPOCH_DAYS) * 86_400.0).round() as i64;
    let (days, time) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Days to the civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let date = format!("{year:04}-{month:02}-{day:02}");
    if time == 0 {
        date
    } else {
        format!(
            "{date} {:02}:{:02}:{:02}",
            time / 3600,
            time % 3600 / 60,
            time % 60
        )
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
    fn excel_dates_as_text() {
        assert_eq!(excel_date_text(45_292.0), "2024-01-01");
        assert_eq!(excel_date_text(45_292.5), "2024-01-01 12:00:00");
        assert_eq!(excel_date_text(25_569.0), "1970-01-01");
        assert_eq!(excel_date_text(60.0), "1900-02-28");
    }

    #[test]
    fn read_options_from_a_connection() {
        let options = BTreeMap::from([
            (ReadOptions::ENCODING.to_owned(), "windows-1252".to_owned()),
            (ReadOptions::SKIP_BAD_LINES.to_owned(), "true".to_owned()),
        ]);
        assert_eq!(
            ReadOptions::of(&options),
            ReadOptions {
                encoding: Some("windows-1252".to_owned()),
                skip_bad_lines: true
            }
        );
        let auto = BTreeMap::from([(ReadOptions::ENCODING.to_owned(), "auto".to_owned())]);
        assert_eq!(ReadOptions::of(&auto), ReadOptions::default());
    }

    /// The rows of `table` in an in-memory database that loaded the file `name` holding
    /// `bytes`, as text; or the load's error.
    fn load_rows(name: &str, bytes: &[u8], options: &ReadOptions) -> Result<Vec<Vec<String>>> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        crate::testing::load();
        let conn = duckdb::Connection::open_in_memory().unwrap();
        load(&conn, path.to_str().unwrap(), options)?;
        let table = Path::new(name).file_stem().unwrap().to_str().unwrap();
        let mut stmt = conn
            .prepare(&format!("SELECT COLUMNS(*)::VARCHAR FROM \"{table}\""))
            .unwrap();
        let n = stmt.column_count();
        let rows = stmt
            .query_map([], |row| {
                (0..n)
                    .map(|i| Ok(row.get::<_, Option<String>>(i)?.unwrap_or("NULL".into())))
                    .collect::<duckdb::Result<Vec<_>>>()
            })
            .unwrap()
            .map(std::result::Result::unwrap)
            .collect();
        Ok(rows)
    }

    #[test]
    fn irregular_csv_lines_fail_or_are_left_out() {
        let ragged = b"a,b\n1,2\n3\n4,5,6\n";
        let err = load_rows("ragged.csv", ragged, &ReadOptions::default()).unwrap_err();
        assert!(err.to_string().contains(BAD_LINES), "{err}");
        assert!(
            err.to_string().contains("line 3 has 1, line 4 has 3"),
            "{err}"
        );
        let skip = ReadOptions {
            skip_bad_lines: true,
            ..ReadOptions::default()
        };
        assert_eq!(
            load_rows("ragged.csv", ragged, &skip).unwrap(),
            [["1", "2"]]
        );
    }

    #[test]
    fn csv_values_as_written() {
        // A quoted empty value is a text, a missing one NULL; long decimals stay exact.
        let rows = load_rows(
            "values.csv",
            b"id,note,amount\n1,\"\",12345678901234567.123456789\n2,,1.5\n",
            &ReadOptions::default(),
        )
        .unwrap();
        assert_eq!(
            rows,
            [
                ["1", "", "12345678901234567.123456789"],
                ["2", "NULL", "1.5"]
            ]
        );
    }

    #[test]
    fn csv_in_other_encodings() {
        let latin = b"id,city\n1,citt\xe0\n2,Mon\xe9\n";
        let rows = load_rows("cities.csv", latin, &ReadOptions::default()).unwrap();
        assert_eq!(rows, [["1", "citt\u{e0}"], ["2", "Mon\u{e9}"]]);
        let utf8 = ReadOptions {
            encoding: Some("utf-8".to_owned()),
            ..ReadOptions::default()
        };
        assert!(load_rows("cities.csv", latin, &utf8).is_err());
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(
                "id;city\n1;citt\u{e0}\n"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes),
            )
            .collect();
        let rows = load_rows("utf16.csv", &utf16, &ReadOptions::default()).unwrap();
        assert_eq!(rows, [["1", "citt\u{e0}"]]);
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
