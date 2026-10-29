//! [`Connection::bulk_load`](platypus_core::Connection::bulk_load) with DuckDB's appender: the
//! rows go to the table's storage without a statement per row, and DuckDB casts each text
//! value to its column's type, as SQL casts it, except the offset of a `TIMESTAMPTZ`, which
//! the appender's cast drops: those values are converted here.

use arrow_schema::DataType;
use duckdb::types::{TimeUnit, Value};
use platypus_core::{BulkRows, Error, Result};
use platypus_sql::{Dialect, Lexer, TokenKind};

use crate::error::map_error;

/// The rows of a [`BulkRows`], owned to cross to the connection's worker.
pub(crate) struct Load {
    /// The table's name parts: `[catalog, [schema,]] name`, unquoted.
    table: Vec<String>,
    /// The query that describes the columns.
    describe: String,
    columns: Vec<String>,
    hex: Vec<bool>,
    rows: Vec<Vec<Option<String>>>,
}

impl Load {
    pub(crate) fn new(rows: &BulkRows<'_>) -> Result<Self> {
        let table = name_parts(rows.table);
        if table.is_empty() || table.len() > 3 {
            return Err(Error::Config(format!("not a table name: {}", rows.table)));
        }
        let columns = rows
            .columns
            .iter()
            .map(|c| match &name_parts(c)[..] {
                [name] => Ok(name.clone()),
                _ => Err(Error::Config(format!("not a column name: {c}"))),
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            table,
            describe: format!(
                "SELECT {} FROM {} LIMIT 0",
                rows.columns.join(", "),
                rows.table
            ),
            columns,
            hex: rows.hex.to_vec(),
            rows: rows.rows.to_vec(),
        })
    }
}

/// The names in a qualified, quoted name (`"main"."t"` is `main` and `t`).
fn name_parts(text: &str) -> Vec<String> {
    Lexer::new(text, Dialect::DuckDb)
        .filter_map(|t| {
            let text = t.text(text);
            match t.kind {
                TokenKind::Word => Some(text.to_owned()),
                TokenKind::QuotedIdent => Some(text[1..text.len() - 1].replace("\"\"", "\"")),
                _ => None,
            }
        })
        .collect()
}

/// Appends `load`'s rows on the worker's connection; the rows written. The appender writes
/// to the table every 2048 rows: outside a transaction (`own_transaction`) the load runs in
/// one of its own, so that a row that fails leaves none of them.
pub(crate) fn append(conn: &duckdb::Connection, load: Load, own_transaction: bool) -> Result<u64> {
    if !own_transaction {
        return append_rows(conn, load);
    }
    conn.execute_batch("BEGIN").map_err(|e| fail(&e))?;
    let appended = append_rows(conn, load);
    let end = if appended.is_ok() {
        "COMMIT"
    } else {
        "ROLLBACK"
    };
    let ended = conn.execute_batch(end).map_err(|e| fail(&e));
    let written = appended?;
    ended?;
    Ok(written)
}

fn fail(e: &duckdb::Error) -> Error {
    map_error(e, "")
}

/// Which of the columns of `load` are `TIMESTAMPTZ`.
fn zoned_columns(conn: &duckdb::Connection, load: &Load) -> Result<Vec<bool>> {
    let mut stmt = conn
        .prepare(&load.describe)
        .map_err(|e| map_error(&e, &load.describe))?;
    let schema = stmt
        .stream_arrow([])
        .map_err(|e| map_error(&e, &load.describe))?
        .get_schema();
    Ok(schema
        .fields()
        .iter()
        .map(|f| matches!(f.data_type(), DataType::Timestamp(_, Some(_))))
        .collect())
}

fn append_rows(conn: &duckdb::Connection, load: Load) -> Result<u64> {
    let zoned = zoned_columns(conn, &load)?;
    let columns: Vec<&str> = load.columns.iter().map(String::as_str).collect();
    let mut appender = match &load.table[..] {
        [name] => conn.appender_with_columns(name, &columns),
        [schema, name] => conn.appender_with_columns_to_db(name, schema, &columns),
        [catalog, schema, name] => {
            conn.appender_with_columns_to_catalog_and_db(name, catalog, schema, &columns)
        }
        _ => unreachable!("checked by Load::new"),
    }
    .map_err(|e| fail(&e))?;
    let written = load.rows.len() as u64;
    for row in load.rows {
        let values = row
            .into_iter()
            .zip(load.hex.iter().zip(&zoned))
            .map(|(value, (&hex, &zoned))| match value {
                None => Ok(Value::Null),
                Some(text) if hex => unhex(&text)
                    .map(Value::Blob)
                    .ok_or_else(|| Error::Config(format!("'{text}' is not hexadecimal bytes"))),
                // Text that does not read (`infinity`) is cast by DuckDB.
                Some(text) if zoned => Ok(utc_micros(&text).map_or(Value::Text(text), |us| {
                    Value::Timestamp(TimeUnit::Microsecond, us)
                })),
                Some(text) => Ok(Value::Text(text)),
            })
            .collect::<Result<Vec<_>>>()?;
        appender
            .append_row(duckdb::appender_params_from_iter(values))
            .map_err(|e| fail(&e))?;
    }
    appender.flush().map_err(|e| fail(&e))?;
    Ok(written)
}

/// Microseconds since 1970 (UTC) of `YYYY-MM-DD[( |T)HH:MM[:SS[.ffffff]]][offset]`, the
/// offset `Z`, `±HH`, `±HHMM` or `±HH:MM` (none is UTC); digits past microseconds are cut.
fn utc_micros(text: &str) -> Option<i64> {
    let text = text.trim();
    let (day, rest) = match text.find([' ', 'T']) {
        Some(at) => (&text[..at], text[at + 1..].trim_start()),
        None => (text, ""),
    };
    let mut ymd = day.splitn(3, '-');
    let (year, month, date): (i64, i64, i64) = (
        ymd.next()?.parse().ok()?,
        ymd.next()?.parse().ok()?,
        ymd.next()?.parse().ok()?,
    );
    if !(1..=12).contains(&month) || !(1..=31).contains(&date) {
        return None;
    }
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == ':' || c == '.'))
        .unwrap_or(rest.len());
    let (clock, offset) = (&rest[..end], rest[end..].trim());
    let (hms, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    // A date alone is midnight.
    let mut parts = hms.split(':').filter(|_| !hms.is_empty());
    let mut part = || -> Option<i64> { parts.next().map_or(Some(0), |p| p.parse().ok()) };
    let (h, m, sec) = (part()?, part()?, part()?);
    if h > 23 || m > 59 || sec > 59 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let micros: i64 = format!("{:0<6}", &fraction[..fraction.len().min(6)])
        .parse()
        .ok()?;
    let offset_minutes = match offset {
        "" | "Z" | "z" => 0,
        _ => {
            let sign = match offset.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let digits: String = offset[1..].chars().filter(|c| *c != ':').collect();
            if !matches!(digits.len(), 2 | 4) || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let minutes = if digits.len() == 4 {
                digits[2..].parse().ok()?
            } else {
                0
            };
            sign * (digits[..2].parse::<i64>().ok()? * 60 + minutes)
        }
    };
    let seconds =
        days_from_civil(year, month, date) * 86_400 + h * 3600 + m * 60 + sec - offset_minutes * 60;
    Some(seconds * 1_000_000 + micros)
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The bytes of hex text (`00ff`).
fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_parts_unquote() {
        assert_eq!(name_parts(r#""main"."a ""b""""#), ["main", r#"a "b""#]);
        assert_eq!(name_parts("t"), ["t"]);
        assert_eq!(name_parts(r#"db."s".t"#), ["db", "s", "t"]);
    }

    #[test]
    fn zoned_timestamps_read_as_utc() {
        let at = |t| utc_micros(t).map(|us| us / 1_000_000);
        assert_eq!(at("1970-01-01 00:00:00"), Some(0));
        assert_eq!(at("1970-01-01T02:00:00+02:00"), Some(0));
        assert_eq!(at("1969-12-31 19:00-05"), Some(0));
        assert_eq!(at("2024-03-01 10:00:00+0530"), Some(1_709_267_400));
        assert_eq!(at("2024-02-29"), Some(1_709_164_800));
        assert_eq!(
            utc_micros("2024-03-01 10:00:00.1234567Z"),
            Some(1_709_287_200_123_456)
        );
        assert_eq!(at("infinity"), None);
        assert_eq!(at("2024-03-01 10:00:00 Europe/Rome"), None);
    }

    #[test]
    fn unhex_reads_pairs() {
        assert_eq!(unhex("00ff"), Some(vec![0, 255]));
        assert_eq!(unhex(""), Some(vec![]));
        assert_eq!(unhex("0"), None);
        assert_eq!(unhex("zz"), None);
    }
}
