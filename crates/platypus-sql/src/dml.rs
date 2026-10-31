//! The `UPDATE`, `INSERT` and `DELETE` statements that save the data editor's changes.
//!
//! Values arrive as text, the way the grid shows them, and are written as literals of the
//! column's type: numbers bare, booleans and binary values in the dialect's syntax, the rest
//! as string literals that the server converts to the column type. Rows are identified by a
//! key (primary key or non-null unique key). Deleted and new rows go in batches of up to
//! [`BATCH_ROWS`] per statement ([`deletes`], [`inserts`]); every statement must change as
//! many rows as it names, which [`apply`] checks while running them all or none.

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

use platypus_core::arrow_schema::{DataType, Field};
use platypus_core::{Connection, Error, ExecOptions, ExecResult, META_DB_TYPE};

use crate::Dialect;

/// How a column's values are written in SQL, from its Arrow type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Bool,
    Integer,
    /// Exact numbers (decimals).
    Decimal,
    Float,
    /// Single-precision floats (`real`): PostgreSQL reads a bare number as a double, which
    /// equals no `real` with a fraction (0.1 is 0.100000001490116 there).
    Real,
    /// Hex text, e.g. `deadbeef`.
    Binary,
    /// `zoned` timestamps are shown with their UTC offset.
    Timestamp {
        zoned: bool,
    },
    Date,
    /// A time of day.
    Time,
    /// Key-value pairs, shown as `{k: 1}`.
    Map,
    Text,
}

impl ValueKind {
    #[must_use]
    pub fn of(data_type: &DataType) -> Self {
        match data_type {
            DataType::Boolean => Self::Bool,
            t if t.is_integer() => Self::Integer,
            DataType::Float16 | DataType::Float32 => Self::Real,
            t if t.is_floating() => Self::Float,
            t if t.is_numeric() => Self::Decimal,
            DataType::Binary
            | DataType::LargeBinary
            | DataType::BinaryView
            | DataType::FixedSizeBinary(_) => Self::Binary,
            DataType::Timestamp(_, tz) => Self::Timestamp {
                zoned: tz.is_some(),
            },
            DataType::Date32 | DataType::Date64 => Self::Date,
            DataType::Time32(_) | DataType::Time64(_) => Self::Time,
            DataType::Map(..) => Self::Map,
            _ => Self::Text,
        }
    }

    /// The kind of a result column. SQLite types a column of a result by the values it holds
    /// (see its driver), so one declared `DECIMAL(10,2)` or `NUMERIC` whose rows are all whole
    /// numbers reads as integers: the declared type decides whether it takes fractions (only a
    /// type with `INT` in its name has integer affinity), and a column with no declared type
    /// takes anything.
    #[must_use]
    pub fn of_field(field: &Field, dialect: Dialect) -> Self {
        let kind = Self::of(field.data_type());
        let whole_only = field
            .metadata()
            .get(META_DB_TYPE)
            .is_some_and(|declared| declared.to_ascii_uppercase().contains("INT"));
        if dialect != Dialect::Sqlite || whole_only {
            return kind;
        }
        // A column with no rows, or only NULLs, is typed text by the driver; its declared
        // `NUMERIC`/`DECIMAL` affinity still turns "1,5" into a mistake, not a string.
        let numeric_declared = field.metadata().get(META_DB_TYPE).is_some_and(|declared| {
            let declared = declared.to_ascii_uppercase();
            declared.contains("NUM") || declared.contains("DEC")
        });
        match kind {
            Self::Integer => Self::Decimal,
            Self::Text if numeric_declared => Self::Decimal,
            _ => kind,
        }
    }
}

/// A value to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value<'a> {
    /// Text as the grid shows it, converted to the column's type.
    Text(&'a str),
    Null,
    /// The column default.
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column<'a> {
    pub name: &'a str,
    pub kind: ValueKind,
    /// The default expression, written where the dialect has no `DEFAULT` keyword.
    pub default: Option<&'a str>,
}

/// A value the column cannot take, e.g. letters in a number column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidValue {
    pub column: String,
    pub message: String,
}

impl fmt::Display for InvalidValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.column, self.message)
    }
}

impl std::error::Error for InvalidValue {}

pub type Result<T, E = InvalidValue> = std::result::Result<T, E>;

/// `[+-]digits[.digits][e[+-]digits]`, or with digits only after the point.
fn is_number(s: &str) -> bool {
    let s = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (mantissa, exponent) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |p: &str| p.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = (!int.is_empty() || !frac.is_empty()) && digits(int) && digits(frac);
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mantissa_ok && exponent_ok
}

/// Digits past which a number keeps its exponent: no exact type holds more.
const MAX_PLAIN_DIGITS: u64 = 1000;

/// A number of [`is_number`] without its exponent, e.g. `-1.5e3` as `-1500`: the same
/// value, exact.
fn plain_number(s: &str) -> String {
    let Some(at) = s.find(['e', 'E']) else {
        return s.to_owned();
    };
    let (mantissa, exponent) = (&s[..at], &s[at + 1..]);
    let Ok(exponent) = exponent.parse::<i64>() else {
        return s.to_owned();
    };
    let (negative, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, mantissa.strip_prefix('+').unwrap_or(mantissa)),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{int}{frac}");
    let Some(point) = i64::try_from(int.len())
        .ok()
        .and_then(|n| n.checked_add(exponent))
    else {
        return s.to_owned();
    };
    if point.unsigned_abs() > MAX_PLAIN_DIGITS {
        return s.to_owned();
    }
    let zeros = |n: i64| "0".repeat(usize::try_from(n).unwrap_or(0));
    let len = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    let (int, frac) = if point <= 0 {
        (String::new(), format!("{}{digits}", zeros(-point)))
    } else if point >= len {
        (format!("{digits}{}", zeros(point - len)), String::new())
    } else {
        let split = usize::try_from(point).unwrap_or(0);
        (digits[..split].to_owned(), digits[split..].to_owned())
    };
    let int = int.trim_start_matches('0');
    let int = if int.is_empty() { "0" } else { int };
    let sign = if negative { "-" } else { "" };
    if frac.is_empty() {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int}.{frac}")
    }
}

/// NaN or an infinity, as floats (and PostgreSQL's numeric) spell them.
fn is_special_number(s: &str) -> bool {
    ["nan", "infinity", "+infinity", "-infinity", "inf", "-inf"]
        .iter()
        .any(|w| w.eq_ignore_ascii_case(s))
}

/// `n` ASCII digits at the start of `s`, and the rest.
fn digits(s: &str, n: usize) -> Option<(&str, &str)> {
    let head = s.get(..n)?;
    head.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| (head, &s[n..]))
}

/// An Oracle ANSI literal of a date or time as the grid and exports write it
/// (`2024-02-29`, `2024-02-29 13:45:59.123456789`, `T` for the space, and for a `zoned` one an
/// offset, `+02:00` or `Z`): `DATE '…'` or `TIMESTAMP '…'`. `None` for other text (a year
/// before Christ, the session's own format), which stays a string.
fn oracle_datetime(text: &str, zoned: bool) -> Option<String> {
    let (year, rest) = digits(text, 4)?;
    let (month, rest) = digits(rest.strip_prefix('-')?, 2)?;
    let (day, rest) = digits(rest.strip_prefix('-')?, 2)?;
    let date = format!("{year}-{month}-{day}");
    if rest.is_empty() {
        return Some(format!("DATE '{date}'"));
    }
    let rest = rest.strip_prefix([' ', 'T'])?;
    let (hour, rest) = digits(rest, 2)?;
    let (minute, rest) = digits(rest.strip_prefix(':')?, 2)?;
    let (second, rest) = match rest.strip_prefix(':') {
        Some(r) => digits(r, 2)?,
        None => ("00", rest),
    };
    let (fraction, rest) = match rest.strip_prefix('.') {
        Some(r) => {
            let n = r.bytes().take_while(u8::is_ascii_digit).count();
            if n == 0 || n > 9 {
                return None;
            }
            (&r[..n], &r[n..])
        }
        None => ("", rest),
    };
    let fraction = if fraction.is_empty() {
        String::new()
    } else {
        format!(".{fraction}")
    };
    let time = format!("{date} {hour}:{minute}:{second}{fraction}");
    let zone = rest.trim();
    let offset = match zone {
        "" => None,
        "Z" | "UTC" | "+00" => Some("+00:00".to_owned()),
        _ if zoned => {
            let (sign, digits_part) = zone.split_at(1);
            if !matches!(sign, "+" | "-") {
                return None;
            }
            let compact = digits_part.replace(':', "");
            let (h, m) = match compact.len() {
                2 => (&compact[..2], "00"),
                4 => (&compact[..2], &compact[2..]),
                _ => return None,
            };
            if !(h.bytes().chain(m.bytes()).all(|b| b.is_ascii_digit())) {
                return None;
            }
            Some(format!("{sign}{h}:{m}"))
        }
        _ => return None,
    };
    Some(match offset {
        Some(offset) => format!("TIMESTAMP '{time} {offset}'"),
        None => format!("TIMESTAMP '{time}'"),
    })
}

/// DuckDB reads a date or timestamp before Christ as `0044-03-15 (BC)` and
/// `0044-03-15 (BC) 12:00:00`; the grid shows `0044-03-15 BC` and `0044-03-15T12:00:00 BC`.
/// `None` for any other text.
fn duckdb_bc(text: &str) -> Option<String> {
    let body = text.strip_suffix(" BC")?;
    let (date, rest) = (body.get(..10)?, body.get(10..)?);
    let rest = rest.strip_prefix(['T', ' ']).unwrap_or(rest);
    Some(if rest.is_empty() {
        format!("{date} (BC)")
    } else {
        format!("{date} (BC) {rest}")
    })
}

/// A map as the grid shows it (`{k: 1, z: 2}`, or DuckDB's `MAP {'k': 1}`) in the form
/// DuckDB casts from text, `{k=1, z=2}`: the separators of the pairs change, the keys and values
/// stay. `None` when a pair has no separator.
fn duckdb_map(text: &str) -> Option<String> {
    let body = text.strip_prefix("MAP").unwrap_or(text).trim_start();
    let inner = body.strip_prefix('{')?.strip_suffix('}')?;
    let (mut depth, mut quote) = (0_u32, None::<char>);
    let (mut pairs, mut key, mut current) = (Vec::new(), None::<String>, String::new());
    for c in inner.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                // Only a quote that starts a key or a value quotes: the text of an unquoted
                // value may hold an apostrophe.
                '\'' | '"' if current.trim().is_empty() => quote = Some(c),
                '[' | '{' | '(' => depth += 1,
                ']' | '}' | ')' => depth = depth.saturating_sub(1),
                ':' | '=' if depth == 0 && key.is_none() => {
                    key = Some(std::mem::take(&mut current));
                    continue;
                }
                ',' if depth == 0 => {
                    pairs.push((key.take()?, std::mem::take(&mut current)));
                    continue;
                }
                _ => {}
            },
        }
        current.push(c);
    }
    if key.is_some() || !current.trim().is_empty() {
        pairs.push((key?, current));
    }
    // An unquoted text with a quote inside is quoted, as DuckDB reads no other.
    let word = |s: &str| {
        let s = s.trim();
        if s.starts_with(['\'', '"']) || !s.contains(['\'', '"']) {
            s.to_owned()
        } else {
            // DuckDB's nested text escapes a quote with a backslash.
            format!("'{}'", s.replace('\'', "\\'"))
        }
    };
    let pairs: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{}={}", word(k), word(v)))
        .collect();
    Some(format!("{{{}}}", pairs.join(", ")))
}

/// More digits than DuckDB reads in a bare integer: such a number is a double there, but a
/// text converts to the column's exact type (`UHUGEINT`).
fn is_wide_integer(number: &str) -> bool {
    let digits = number.strip_prefix(['+', '-']).unwrap_or(number);
    digits.len() > 38 && digits.bytes().all(|b| b.is_ascii_digit())
}

fn is_integer(s: &str) -> bool {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

fn parse_bool(s: &str) -> Option<bool> {
    const TRUE: &[&str] = &["true", "t", "1", "yes", "y", "on"];
    const FALSE: &[&str] = &["false", "f", "0", "no", "n", "off"];
    if TRUE.iter().any(|w| w.eq_ignore_ascii_case(s)) {
        Some(true)
    } else if FALSE.iter().any(|w| w.eq_ignore_ascii_case(s)) {
        Some(false)
    } else {
        None
    }
}

/// Hex digits, with an optional `\x` or `0x` prefix, lower-cased.
fn parse_hex(s: &str) -> Option<String> {
    let hex = s
        .strip_prefix("\\x")
        .or_else(|| s.strip_prefix("0x"))
        .unwrap_or(s);
    (hex.len().is_multiple_of(2) && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

impl Column<'_> {
    fn invalid(&self, message: impl Into<String>) -> InvalidValue {
        InvalidValue {
            column: self.name.to_owned(),
            message: message.into(),
        }
    }

    /// `text` as a bulk load reads it (PostgreSQL's `COPY`, MySQL's `LOAD DATA`): the value
    /// of [`literal`](Self::literal) without its quotes, checked as it does; bytes as hex.
    ///
    /// # Errors
    /// When `text` is not a value of the column's kind.
    pub fn bulk_text(&self, dialect: Dialect, text: &str) -> Result<String> {
        let trimmed = text.trim();
        Ok(match self.kind {
            ValueKind::Integer if is_integer(trimmed) => trimmed.to_owned(),
            ValueKind::Integer => {
                return Err(self.invalid(format!("'{text}' is not a whole number")));
            }
            ValueKind::Decimal | ValueKind::Float | ValueKind::Real if is_number(trimmed) => {
                plain_number(trimmed)
            }
            // PostgreSQL reads NaN and the infinities of floats (and numerics since 14).
            ValueKind::Decimal | ValueKind::Float | ValueKind::Real
                if dialect == Dialect::Postgres && is_special_number(trimmed) =>
            {
                trimmed.to_owned()
            }
            ValueKind::Decimal | ValueKind::Float | ValueKind::Real => {
                return Err(self.invalid(format!("'{text}' is not a number")));
            }
            ValueKind::Bool => match (parse_bool(trimmed), dialect) {
                (Some(b), Dialect::MySql) => if b { "1" } else { "0" }.to_owned(),
                (Some(b), _) => b.to_string(),
                (None, _) => return Err(self.invalid(format!("'{text}' is not true or false"))),
            },
            ValueKind::Binary => parse_hex(trimmed).ok_or_else(|| {
                self.invalid(format!("'{text}' is not hexadecimal bytes (e.g. 0a1b)"))
            })?,
            // MySQL sessions run in UTC and MariaDB rejects offsets in datetimes.
            ValueKind::Timestamp { zoned: true } if dialect == Dialect::MySql => trimmed
                .strip_suffix("+00:00")
                .or_else(|| trimmed.strip_suffix('Z'))
                .unwrap_or(trimmed)
                .to_owned(),
            ValueKind::Timestamp { .. } | ValueKind::Date if dialect == Dialect::DuckDb => {
                duckdb_bc(trimmed).unwrap_or_else(|| trimmed.to_owned())
            }
            ValueKind::Timestamp { .. } | ValueKind::Date | ValueKind::Time => trimmed.to_owned(),
            ValueKind::Map if dialect == Dialect::DuckDb => {
                duckdb_map(trimmed).unwrap_or_else(|| text.to_owned())
            }
            ValueKind::Map | ValueKind::Text => text.to_owned(),
        })
    }

    /// `text` as a literal of the column's type; `None` is NULL.
    pub fn literal(&self, dialect: Dialect, text: Option<&str>) -> Result<String> {
        let Some(text) = text else {
            return Ok("NULL".to_owned());
        };
        let trimmed = text.trim();
        Ok(match self.kind {
            ValueKind::Integer if is_integer(trimmed) => trimmed.to_owned(),
            ValueKind::Integer => {
                return Err(self.invalid(format!("'{text}' is not a whole number")));
            }
            // Exact digits: a `VALUES` list with an exponent in it can turn the column float.
            ValueKind::Decimal if dialect == Dialect::DuckDb && is_wide_integer(trimmed) => {
                dialect.quote_literal(trimmed)
            }
            ValueKind::Decimal if is_number(trimmed) => plain_number(trimmed),
            // PostgreSQL's numeric has NaN and (since 14) infinities.
            ValueKind::Decimal if dialect == Dialect::Postgres && is_special_number(trimmed) => {
                format!("{}::numeric", dialect.quote_literal(trimmed))
            }
            // Read as a `real`, the shortest text of a float4 is that float4 again.
            ValueKind::Real
                if dialect == Dialect::Postgres
                    && (is_number(trimmed) || is_special_number(trimmed)) =>
            {
                format!("{}::real", dialect.quote_literal(trimmed))
            }
            ValueKind::Float | ValueKind::Real if is_number(trimmed) => trimmed.to_owned(),
            ValueKind::Float | ValueKind::Real if is_special_number(trimmed) => {
                dialect.quote_literal(trimmed)
            }
            ValueKind::Decimal | ValueKind::Float | ValueKind::Real => {
                return Err(self.invalid(format!("'{text}' is not a number")));
            }
            ValueKind::Bool => match parse_bool(trimmed) {
                Some(b) => dialect.bool_literal(b).to_owned(),
                None => return Err(self.invalid(format!("'{text}' is not true or false"))),
            },
            ValueKind::Binary => match parse_hex(trimmed) {
                Some(hex) => dialect.binary_literal(&hex),
                None => {
                    return Err(
                        self.invalid(format!("'{text}' is not hexadecimal bytes (e.g. 0a1b)"))
                    );
                }
            },
            // MySQL sessions run in UTC and MariaDB rejects offsets in datetime literals.
            ValueKind::Timestamp { zoned: true } if dialect == Dialect::MySql => {
                let local = trimmed
                    .strip_suffix("+00:00")
                    .or_else(|| trimmed.strip_suffix('Z'))
                    .unwrap_or(trimmed);
                dialect.quote_literal(local)
            }
            // ClickHouse reads no offset in a DateTime literal, and a Nullable column takes
            // NULL instead: the instant is parsed, at the precision of its fraction.
            ValueKind::Timestamp { zoned: true } if dialect == Dialect::ClickHouse => {
                let fraction = trimmed
                    .split_once('.')
                    .map_or(0, |(_, f)| f.bytes().take_while(u8::is_ascii_digit).count());
                format!(
                    "parseDateTime64BestEffort({}, {})",
                    dialect.quote_literal(trimmed),
                    if fraction > 6 { 9 } else { 6 }
                )
            }
            // Oracle reads a string as a time with the session's NLS formats: an ANSI literal
            // does not depend on them.
            ValueKind::Timestamp { zoned } if dialect == Dialect::Oracle => {
                oracle_datetime(trimmed, zoned).unwrap_or_else(|| dialect.quote_literal(text))
            }
            ValueKind::Date if dialect == Dialect::Oracle => {
                oracle_datetime(trimmed, false).unwrap_or_else(|| dialect.quote_literal(text))
            }
            // Standard SQL's typed literals: Trino converts no text to a date or time, not
            // even on INSERT.
            ValueKind::Date if dialect == Dialect::Ansi => {
                format!("DATE {}", dialect.quote_literal(trimmed))
            }
            ValueKind::Time if dialect == Dialect::Ansi => {
                format!("TIME {}", dialect.quote_literal(trimmed))
            }
            ValueKind::Timestamp { .. } if dialect == Dialect::Ansi => {
                // `2024-01-02T03:04:05` as Arrow writes it, `2024-01-02 03:04:05` in SQL.
                let sql = match trimmed.split_at_checked(10) {
                    Some((day, rest)) if rest.starts_with('T') => format!("{day} {}", &rest[1..]),
                    _ => trimmed.to_owned(),
                };
                format!("TIMESTAMP {}", dialect.quote_literal(&sql))
            }
            ValueKind::Timestamp { .. } | ValueKind::Date if dialect == Dialect::DuckDb => {
                dialect.quote_literal(&duckdb_bc(trimmed).unwrap_or_else(|| text.to_owned()))
            }
            ValueKind::Map if dialect == Dialect::DuckDb => {
                dialect.quote_literal(&duckdb_map(trimmed).unwrap_or_else(|| text.to_owned()))
            }
            ValueKind::Timestamp { .. }
            | ValueKind::Date
            | ValueKind::Time
            | ValueKind::Map
            | ValueKind::Text => dialect.quote_literal(text),
        })
    }

    /// The right-hand side of `SET column = …`.
    fn assignment(&self, dialect: Dialect, value: Value<'_>) -> Result<String> {
        match value {
            Value::Text(text) => self.literal(dialect, Some(text)),
            Value::Null => self.literal(dialect, None),
            Value::Default if dialect.has_default_keyword() => Ok("DEFAULT".to_owned()),
            Value::Default => Ok(self.default.unwrap_or("NULL").to_owned()),
        }
    }
}

fn column<'c, 'a>(columns: &'c [Column<'a>], index: usize) -> Result<&'c Column<'a>> {
    columns.get(index).ok_or_else(|| InvalidValue {
        column: format!("#{index}"),
        message: "no such column".to_owned(),
    })
}

/// The empty key's error: nothing tells the row apart.
fn no_key() -> InvalidValue {
    InvalidValue {
        column: String::new(),
        message: "the row has no key to identify it".to_owned(),
    }
}

/// `k1 = … AND k2 IS NULL` into `out`; `key` pairs column indexes with the row's values.
fn key_match(
    dialect: Dialect,
    columns: &[Column<'_>],
    key: &[(usize, Option<&str>)],
    out: &mut String,
) -> Result<()> {
    for (i, &(index, value)) in key.iter().enumerate() {
        let c = column(columns, index)?;
        if i > 0 {
            out.push_str(" AND ");
        }
        out.push_str(&dialect.quote_ident(c.name));
        if value.is_none() {
            out.push_str(" IS NULL");
        } else {
            out.push_str(" = ");
            out.push_str(&c.literal(dialect, value)?);
        }
    }
    Ok(())
}

/// `WHERE k1 = … AND k2 IS NULL`; `key` pairs column indexes with the row's current values.
fn where_clause(
    dialect: Dialect,
    columns: &[Column<'_>],
    key: &[(usize, Option<&str>)],
) -> Result<String> {
    if key.is_empty() {
        return Err(no_key());
    }
    let mut out = String::from(" WHERE ");
    key_match(dialect, columns, key, &mut out)?;
    Ok(out)
}

/// `UPDATE table SET … WHERE key`; `table` is quoted already, e.g. by [`Dialect::qualified`].
pub fn update(
    dialect: Dialect,
    table: &str,
    columns: &[Column<'_>],
    set: &[(usize, Value<'_>)],
    key: &[(usize, Option<&str>)],
) -> Result<String> {
    let mut sql = format!("UPDATE {table} SET ");
    for (i, &(index, value)) in set.iter().enumerate() {
        let c = column(columns, index)?;
        if i > 0 {
            sql.push_str(", ");
        }
        sql.push_str(&dialect.quote_ident(c.name));
        sql.push_str(" = ");
        sql.push_str(&c.assignment(dialect, value)?);
    }
    sql.push_str(&where_clause(dialect, columns, key)?);
    Ok(sql)
}

/// The columns of a new row, as [`insert_row`] writes them.
struct InsertRow<'c> {
    names: Vec<Cow<'c, str>>,
    literals: Vec<String>,
    /// It gives a value to one of the `identity` columns of [`insert_row`].
    identity: bool,
}

/// The quoted names and the literals of the columns a new row sets: those left at their
/// default are left out. `identity` lists the columns the database numbers, which take a
/// value only with an override (see [`restore_inserts`]).
fn insert_row<'c>(
    dialect: Dialect,
    columns: &'c [Column<'_>],
    values: &[(usize, Value<'_>)],
    identity: &[usize],
) -> Result<InsertRow<'c>> {
    let mut row = InsertRow {
        names: Vec::with_capacity(values.len()),
        literals: Vec::with_capacity(values.len()),
        identity: false,
    };
    for &(index, value) in values {
        let c = column(columns, index)?;
        let text = match value {
            Value::Default => continue,
            Value::Null => None,
            Value::Text(text) => Some(text),
        };
        row.identity |= identity.contains(&index);
        row.names.push(dialect.quote_ident(c.name));
        row.literals.push(c.literal(dialect, text)?);
    }
    Ok(row)
}

/// `INSERT INTO table DEFAULT VALUES`, or the dialect's form of it.
fn insert_defaults(dialect: Dialect, table: &str, columns: &[Column<'_>]) -> String {
    let any = columns
        .first()
        .map_or(Cow::Borrowed(""), |c| dialect.quote_ident(c.name));
    dialect.insert_defaults(table, &any)
}

/// What comes between the column list and `VALUES`: PostgreSQL takes values for identity
/// columns `GENERATED ALWAYS` only when told to.
const fn overriding(dialect: Dialect, identity: bool) -> &'static str {
    if identity && matches!(dialect, Dialect::Postgres) {
        " OVERRIDING SYSTEM VALUE"
    } else {
        ""
    }
}

/// `INSERT INTO table (…) VALUES (…)`. Columns left at their default are omitted.
pub fn insert(
    dialect: Dialect,
    table: &str,
    columns: &[Column<'_>],
    values: &[(usize, Value<'_>)],
) -> Result<String> {
    let row = insert_row(dialect, columns, values, &[])?;
    if row.names.is_empty() {
        return Ok(insert_defaults(dialect, table, columns));
    }
    Ok(format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        row.names.join(", "),
        row.literals.join(", ")
    ))
}

/// `DELETE FROM table WHERE key`.
pub fn delete(
    dialect: Dialect,
    table: &str,
    columns: &[Column<'_>],
    key: &[(usize, Option<&str>)],
) -> Result<String> {
    Ok(format!(
        "DELETE FROM {table}{}",
        where_clause(dialect, columns, key)?
    ))
}

/// Most rows a batched statement names: lists stay well inside every database's limits
/// (Oracle takes 1,000 expressions in a list, SQL Server 1,000 rows in `VALUES`).
pub const BATCH_ROWS: usize = 500;

/// Most bytes of SQL a batched statement grows to: well under MySQL's default
/// `max_allowed_packet` (4 MiB) and the request limits of drivers over HTTP.
pub const BATCH_BYTES: usize = 1 << 20;

/// A statement that saving runs, and the rows it must change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub sql: String,
    /// The rows it names. Any other count means some of them changed or disappeared since
    /// they were loaded, or the key does not tell rows apart.
    pub rows: u64,
}

impl Statement {
    /// A statement of one row.
    #[must_use]
    pub const fn one(sql: String) -> Self {
        Self { sql, rows: 1 }
    }

    fn of(sql: String, rows: usize) -> Self {
        Self {
            sql,
            rows: u64::try_from(rows).unwrap_or(u64::MAX),
        }
    }
}

/// How a statement lists rows by a key of several columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowList {
    /// `(k1, k2) IN ((…), (…))`.
    Tuples,
    /// `(k1, k2) IN (VALUES (…), (…))`: SQLite before 3.37 takes row values on the right
    /// of `IN` only from a subquery.
    Values,
    /// `(k1 = … AND k2 = …) OR …`, which every database takes. DuckDB compares row values
    /// as structs, which can miss timestamps, decimals and UUIDs written as text; older
    /// MySQL servers scan the table for row values.
    Or,
}

const fn row_list(dialect: Dialect) -> RowList {
    match dialect {
        Dialect::Postgres => RowList::Tuples,
        Dialect::Sqlite => RowList::Values,
        _ => RowList::Or,
    }
}

/// Whether `INSERT … VALUES` takes several rows: Oracle's does not before 23ai, nor do some
/// databases behind generic ODBC and ADBC drivers.
const fn has_multi_row_values(dialect: Dialect) -> bool {
    !matches!(dialect, Dialect::Oracle | Dialect::Ansi)
}

/// Splits items of the given sizes into consecutive runs of at most [`BATCH_ROWS`] items
/// and, unless a run is a single item, [`BATCH_BYTES`] bytes.
fn batch_runs(sizes: impl IntoIterator<Item = usize>) -> Vec<Range<usize>> {
    let mut runs = Vec::new();
    let (mut start, mut bytes) = (0, 0);
    let mut end = 0;
    for size in sizes {
        if end > start && (end - start == BATCH_ROWS || bytes + size > BATCH_BYTES) {
            runs.push(start..end);
            (start, bytes) = (end, 0);
        }
        bytes += size;
        end += 1;
    }
    if end > start {
        runs.push(start..end);
    }
    runs
}

/// A key as an item of the list of [`key_list`]: its literals, or for [`RowList::Or`] its
/// `k1 = … AND k2 = …` in parentheses.
fn key_item(
    dialect: Dialect,
    columns: &[Column<'_>],
    key: &[(usize, Option<&str>)],
) -> Result<String> {
    let mut out = String::new();
    let composite = key.len() > 1;
    if composite && row_list(dialect) == RowList::Or {
        out.push('(');
        key_match(dialect, columns, key, &mut out)?;
        out.push(')');
        return Ok(out);
    }
    if composite {
        out.push('(');
    }
    for (i, &(index, value)) in key.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&column(columns, index)?.literal(dialect, value)?);
    }
    if composite {
        out.push(')');
    }
    Ok(out)
}

/// The condition matching any of the keys of `items` (from [`key_item`]), whose columns
/// are those of `key`.
fn key_list(
    dialect: Dialect,
    columns: &[Column<'_>],
    key: &[(usize, Option<&str>)],
    items: &[String],
) -> Result<String> {
    let names = key
        .iter()
        .map(|&(index, _)| Ok(dialect.quote_ident(column(columns, index)?.name)))
        .collect::<Result<Vec<_>>>()?;
    Ok(match (names.as_slice(), row_list(dialect)) {
        ([name], _) => format!("{name} IN ({})", items.join(", ")),
        (_, RowList::Or) => items.join(" OR "),
        (_, RowList::Tuples) => format!("({}) IN ({})", names.join(", "), items.join(", ")),
        (_, RowList::Values) => {
            format!("({}) IN (VALUES {})", names.join(", "), items.join(", "))
        }
    })
}

/// The `DELETE` statements of the rows with `keys`, each pairing the key columns with a
/// row's values: up to [`BATCH_ROWS`] rows and [`BATCH_BYTES`] bytes per statement. A key
/// with a NULL value, which no list matches, gets a statement of its own with `IS NULL`.
///
/// [`apply`] checks a statement's total of rows only, so the key must be unique (a primary
/// or unique key): with duplicates, a missing row and an extra one could make up the count.
pub fn deletes(
    dialect: Dialect,
    table: &str,
    columns: &[Column<'_>],
    keys: &[Vec<(usize, Option<&str>)>],
) -> Result<Vec<Statement>> {
    let same_columns = |a: &[(usize, Option<&str>)], b: &[(usize, Option<&str>)]| {
        a.iter().map(|&(c, _)| c).eq(b.iter().map(|&(c, _)| c))
    };
    let first = keys.first().map_or(&[][..], Vec::as_slice);
    let (listed, single): (Vec<&[_]>, Vec<&[_]>) =
        keys.iter().map(Vec::as_slice).partition(|key| {
            !key.is_empty() && key.iter().all(|(_, v)| v.is_some()) && same_columns(first, key)
        });
    let items = listed
        .iter()
        .map(|key| key_item(dialect, columns, key))
        .collect::<Result<Vec<_>>>()?;
    let runs = batch_runs(items.iter().map(String::len));
    let mut statements = Vec::with_capacity(runs.len() + single.len());
    for run in runs {
        let sql = if run.len() == 1 {
            delete(dialect, table, columns, listed[run.start])?
        } else {
            let list = key_list(dialect, columns, first, &items[run.clone()])?;
            format!("DELETE FROM {table} WHERE {list}")
        };
        statements.push(Statement::of(sql, run.len()));
    }
    for key in single {
        statements.push(Statement::one(delete(dialect, table, columns, key)?));
    }
    Ok(statements)
}

/// Whether a literal is a quoted string (`'…'`, `N'…'`, `E'…'`); `None` for NULL, which
/// goes with either. A `VALUES` list unifies each column's type across its rows, and some
/// databases fail on, or round, numbers mixed with strings (`'NaN'`).
fn literal_form(literal: &str) -> Option<bool> {
    (literal != "NULL").then(|| literal.trim_start_matches(['N', 'E']).starts_with('\''))
}

/// An `INSERT` being filled with rows.
struct OpenInsert<'c> {
    names: Vec<Cow<'c, str>>,
    /// [`overriding`] of the rows, which set the same columns.
    overriding: &'static str,
    /// [`literal_form`] by column, of the rows so far.
    forms: Vec<Option<bool>>,
    values: String,
    rows: usize,
}

impl OpenInsert<'_> {
    fn takes(&self, names: &[Cow<'_, str>], forms: &[Option<bool>], tuple: &str) -> bool {
        self.names == names
            && self.values.len() + tuple.len() <= BATCH_BYTES
            && self
                .forms
                .iter()
                .zip(forms)
                .all(|(a, b)| a.is_none() || b.is_none() || a == b)
    }

    fn statement(self, table: &str) -> Statement {
        let sql = format!(
            "INSERT INTO {table} ({}){} VALUES {}",
            self.names.join(", "),
            self.overriding,
            self.values
        );
        Statement::of(sql, self.rows)
    }
}

/// The `INSERT` statements of new `rows`, in their order: consecutive rows that set the
/// same columns, with literals of the same form, share a `VALUES` list of up to
/// [`BATCH_ROWS`] rows and [`BATCH_BYTES`] bytes, where the dialect takes several rows.
pub fn inserts(
    dialect: Dialect,
    table: &str,
    columns: &[Column<'_>],
    rows: &[Vec<(usize, Value<'_>)>],
) -> Result<Vec<Statement>> {
    batched_inserts(dialect, table, columns, rows, &[]).map(|(statements, _)| statements)
}

/// The statements that put deleted `rows` back, for an undo script: [`inserts`] that give
/// the `identity` columns (numbered by the database, which takes a value for them only when
/// told to) their old values, with `OVERRIDING SYSTEM VALUE` (PostgreSQL) or between `SET
/// IDENTITY_INSERT … ON` and `OFF` (SQL Server).
pub fn restore_inserts(
    dialect: Dialect,
    table: &str,
    columns: &[Column<'_>],
    rows: &[Vec<(usize, Value<'_>)>],
    identity: &[usize],
) -> Result<Vec<String>> {
    let (statements, any_identity) = batched_inserts(dialect, table, columns, rows, identity)?;
    let mut sql: Vec<String> = statements.into_iter().map(|s| s.sql).collect();
    if any_identity && dialect == Dialect::SqlServer {
        sql.insert(0, format!("SET IDENTITY_INSERT {table} ON"));
        sql.push(format!("SET IDENTITY_INSERT {table} OFF"));
    }
    Ok(sql)
}

/// The statements of [`inserts`] that may set the `identity` columns, and whether any does.
fn batched_inserts<'c>(
    dialect: Dialect,
    table: &str,
    columns: &'c [Column<'_>],
    rows: &[Vec<(usize, Value<'_>)>],
    identity: &[usize],
) -> Result<(Vec<Statement>, bool)> {
    let limit = if has_multi_row_values(dialect) {
        BATCH_ROWS
    } else {
        1
    };
    let mut statements = Vec::new();
    let mut any_identity = false;
    let mut open: Option<OpenInsert<'c>> = None;
    for values in rows {
        let InsertRow {
            names,
            literals,
            identity: sets_identity,
        } = insert_row(dialect, columns, values, identity)?;
        any_identity |= sets_identity;
        let tuple = format!("({})", literals.join(", "));
        let forms: Vec<_> = literals.iter().map(|l| literal_form(l)).collect();
        if open
            .as_ref()
            .is_some_and(|o| o.rows == limit || !o.takes(&names, &forms, &tuple))
        {
            statements.extend(open.take().map(|o| o.statement(table)));
        }
        if names.is_empty() {
            statements.push(Statement::one(insert_defaults(dialect, table, columns)));
            continue;
        }
        let o = open.get_or_insert_with(|| OpenInsert {
            names,
            overriding: overriding(dialect, sets_identity),
            forms: vec![None; forms.len()],
            values: String::new(),
            rows: 0,
        });
        for (form, new) in o.forms.iter_mut().zip(forms) {
            *form = form.or(new);
        }
        if o.rows > 0 {
            o.values.push_str(", ");
        }
        o.values.push_str(&tuple);
        o.rows += 1;
    }
    statements.extend(open.map(|o| o.statement(table)));
    Ok((statements, any_identity))
}

/// Why [`apply`] rolled back.
#[derive(Debug)]
pub enum ApplyError {
    /// A statement failed; `index` is its position in the list.
    Failed { index: usize, error: Error },
    /// A statement hit `rows` rows instead of the `expected` ones it names: they changed or
    /// disappeared since they were loaded, or the key is not unique after all.
    RowCount {
        index: usize,
        rows: u64,
        expected: u64,
    },
    /// The save failed in manual-commit mode where the database has no savepoints: the
    /// session's whole transaction was rolled back, its other uncommitted changes with it.
    SessionRolledBack(Box<ApplyError>),
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::SessionRolledBack(ref cause) => write!(
                f,
                "{cause}. The database has no savepoints, so the whole transaction was rolled \
                 back, with the other changes not committed yet"
            ),
            Self::Failed { index, ref error } => write!(f, "statement {}: {error}", index + 1),
            Self::RowCount {
                index,
                rows: 0,
                expected: 1,
            } => write!(
                f,
                "statement {}: the row was not found; it may have been changed or deleted since it was loaded",
                index + 1
            ),
            Self::RowCount {
                index,
                rows,
                expected: 1,
            } => write!(
                f,
                "statement {} would change {rows} rows instead of one",
                index + 1
            ),
            Self::RowCount {
                index,
                rows,
                expected,
            } if rows < expected => write!(
                f,
                "statement {}: only {rows} of its {expected} rows were found; the others may have been changed or deleted since they were loaded",
                index + 1
            ),
            Self::RowCount {
                index,
                rows,
                expected,
            } => write!(
                f,
                "statement {} would change {rows} rows instead of {expected}",
                index + 1
            ),
        }
    }
}

impl std::error::Error for ApplyError {}

async fn exec_one(conn: &dyn Connection, sql: &str) -> Result<u64, Error> {
    match conn.execute(sql, &ExecOptions::default()).await? {
        ExecResult::Affected { rows } => Ok(rows),
        // Dropping the stream ends the statement.
        ExecResult::Rows(_) => Ok(0),
    }
}

const SAVEPOINT: &str = "platypus_edit";

/// The statements around a save: the one that opens it, those that keep it and those that
/// undo it.
fn bracket(dialect: Dialect, manual: bool) -> (Option<String>, Vec<String>, Vec<String>) {
    if !manual {
        return (
            Some("BEGIN".to_owned()),
            vec!["COMMIT".to_owned()],
            vec!["ROLLBACK".to_owned()],
        );
    }
    if !dialect.has_savepoints() {
        return (None, Vec::new(), vec!["ROLLBACK".to_owned()]);
    }
    // Oracle's savepoints last until the transaction ends.
    let release = dialect
        .releases_savepoints()
        .then(|| format!("RELEASE SAVEPOINT {SAVEPOINT}"));
    let undo = std::iter::once(format!("ROLLBACK TO SAVEPOINT {SAVEPOINT}"))
        .chain(release.clone())
        .collect();
    (
        Some(format!("SAVEPOINT {SAVEPOINT}")),
        release.into_iter().collect(),
        undo,
    )
}

/// Runs `statements`, each of which must change the rows it names, all or none. They go
/// to the server together where the driver can send them so ([`Connection::execute_all`]);
/// otherwise they stop at the first row count off. In autocommit mode they run in a
/// transaction of their own and are committed; in manual-commit mode they join the
/// session's transaction behind a savepoint, and stay uncommitted.
///
/// Without savepoints (DuckDB, Db2, Snowflake…) a failure in manual-commit mode rolls back
/// the session's whole transaction, as nothing else can undo the statements before it, and
/// says so ([`ApplyError::SessionRolledBack`]); a save that changed nothing yet leaves it
/// as it was.
pub async fn apply(
    conn: &dyn Connection,
    dialect: Dialect,
    statements: &[Statement],
) -> Result<(), ApplyError> {
    let manual = !conn.transaction().autocommit;
    let savepointless = manual && !dialect.has_savepoints();
    let (begin, end, undo) = bracket(dialect, manual);
    if let Some(begin) = begin {
        exec_one(conn, &begin)
            .await
            .map_err(|error| ApplyError::Failed { index: 0, error })?;
    }
    let sql: Vec<&str> = statements.iter().map(|s| s.sql.as_str()).collect();
    let expected: Vec<u64> = statements.iter().map(|s| s.rows).collect();
    let ran = conn.execute_all(&sql, &expected).await;
    let mismatch = ran
        .rows
        .iter()
        .zip(statements)
        .enumerate()
        .find(|(_, (rows, s))| **rows != s.rows);
    let untouched = ran.error.is_none() && ran.rows.iter().all(|&rows| rows == 0);
    let outcome = match (mismatch, ran.error) {
        (Some((index, (&rows, s))), _) => Err(ApplyError::RowCount {
            index,
            rows,
            expected: s.rows,
        }),
        (None, Some(error)) => Err(ApplyError::Failed {
            index: ran.rows.len(),
            error,
        }),
        (None, None) => Ok(()),
    };
    let finish = match outcome {
        Ok(()) => end,
        Err(_) if savepointless && untouched => Vec::new(),
        Err(_) => undo,
    };
    for sql in &finish {
        if let Err(error) = exec_one(conn, sql).await {
            // The first error explains more than the failed cleanup.
            return outcome.and(Err(ApplyError::Failed {
                index: statements.len(),
                error,
            }));
        }
    }
    match outcome {
        Err(e) if savepointless && !untouched => Err(ApplyError::SessionRolledBack(Box::new(e))),
        outcome => outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulk_text_is_the_literal_without_quotes() {
        let column = |kind| Column {
            name: "c",
            kind,
            default: None,
        };
        let pg = Dialect::Postgres;
        let my = Dialect::MySql;
        assert_eq!(
            column(ValueKind::Integer).bulk_text(pg, " 42 ").unwrap(),
            "42"
        );
        assert!(column(ValueKind::Integer).bulk_text(pg, "4x").is_err());
        assert_eq!(
            column(ValueKind::Decimal).bulk_text(pg, "1e2").unwrap(),
            "100"
        );
        assert_eq!(
            column(ValueKind::Float).bulk_text(pg, "NaN").unwrap(),
            "NaN"
        );
        assert!(column(ValueKind::Float).bulk_text(my, "NaN").is_err());
        assert_eq!(
            column(ValueKind::Bool).bulk_text(pg, "yes").unwrap(),
            "true"
        );
        assert_eq!(column(ValueKind::Bool).bulk_text(my, "false").unwrap(), "0");
        assert_eq!(
            column(ValueKind::Binary).bulk_text(pg, "0xDEAD").unwrap(),
            "dead"
        );
        let zoned = column(ValueKind::Timestamp { zoned: true });
        assert_eq!(
            zoned.bulk_text(my, "2026-01-02T03:04:05Z").unwrap(),
            "2026-01-02T03:04:05"
        );
        assert_eq!(
            column(ValueKind::Text).bulk_text(pg, " as is ").unwrap(),
            " as is "
        );
    }

    const fn col(name: &str, kind: ValueKind) -> Column<'_> {
        Column {
            name,
            kind,
            default: None,
        }
    }

    fn table(dialect: Dialect) -> String {
        dialect.qualified(Some("demo"), "Items")
    }

    fn columns() -> Vec<Column<'static>> {
        vec![
            col("id", ValueKind::Integer),
            col("name", ValueKind::Text),
            col("price", ValueKind::Decimal),
            col("active", ValueKind::Bool),
            col("data", ValueKind::Binary),
            col("seen", ValueKind::Timestamp { zoned: true }),
            Column {
                name: "order",
                kind: ValueKind::Integer,
                default: Some("0"),
            },
        ]
    }

    #[test]
    fn sqlite_numeric_columns_take_fractions_whatever_they_hold() {
        use std::collections::HashMap;
        let field = |declared: Option<&str>| {
            let f = Field::new("price", DataType::Int64, true);
            match declared {
                Some(d) => {
                    f.with_metadata(HashMap::from([(META_DB_TYPE.to_owned(), d.to_owned())]))
                }
                None => f,
            }
        };
        let kind = |declared, dialect| ValueKind::of_field(&field(declared), dialect);
        for numeric in [Some("DECIMAL(10,2)"), Some("NUMERIC"), Some("REAL"), None] {
            assert_eq!(
                kind(numeric, Dialect::Sqlite),
                ValueKind::Decimal,
                "{numeric:?}"
            );
        }
        // Declared numeric but typed text by the driver (no rows): still a number.
        for declared in ["NUMERIC", "DECIMAL(10,2)", "numeric(5)"] {
            let text = Field::new("price", DataType::Utf8, true).with_metadata(HashMap::from([(
                META_DB_TYPE.to_owned(),
                declared.to_owned(),
            )]));
            assert_eq!(
                ValueKind::of_field(&text, Dialect::Sqlite),
                ValueKind::Decimal,
                "{declared}"
            );
        }
        let text = Field::new("name", DataType::Utf8, true).with_metadata(HashMap::from([(
            META_DB_TYPE.to_owned(),
            "TEXT".to_owned(),
        )]));
        assert_eq!(ValueKind::of_field(&text, Dialect::Sqlite), ValueKind::Text);
        for whole in ["INTEGER", "bigint", "UNSIGNED BIG INT"] {
            assert_eq!(
                kind(Some(whole), Dialect::Sqlite),
                ValueKind::Integer,
                "{whole}"
            );
        }
        assert_eq!(
            kind(Some("DECIMAL(10,2)"), Dialect::Postgres),
            ValueKind::Integer
        );
        let price = Column {
            name: "price",
            kind: ValueKind::Decimal,
            default: None,
        };
        assert_eq!(
            price.literal(Dialect::Sqlite, Some("9.99")).unwrap(),
            "9.99"
        );
    }

    #[test]
    fn duckdb_reads_maps_dates_and_wide_numbers_as_they_are_shown() {
        let d = Dialect::DuckDb;
        let lit = |kind, text| col("c", kind).literal(d, Some(text)).unwrap();
        assert_eq!(lit(ValueKind::Map, "{k: 1, z: 2}"), "'{k=1, z=2}'");
        assert_eq!(
            lit(ValueKind::Map, "{'a b': 'x, y', c: NULL}"),
            "'{''a b''=''x, y'', c=NULL}'"
        );
        assert_eq!(lit(ValueKind::Map, "MAP {'k': 1}"), "'{''k''=1}'");
        assert_eq!(lit(ValueKind::Map, "{k=1}"), "'{k=1}'");
        assert_eq!(lit(ValueKind::Map, "{}"), "'{}'");
        assert_eq!(lit(ValueKind::Map, "nope"), "'nope'");
        assert_eq!(lit(ValueKind::Date, "0044-03-15 BC"), "'0044-03-15 (BC)'");
        assert_eq!(
            lit(
                ValueKind::Timestamp { zoned: false },
                "0044-03-15T12:00:00.5 BC"
            ),
            "'0044-03-15 (BC) 12:00:00.5'"
        );
        assert_eq!(lit(ValueKind::Date, "2024-01-01"), "'2024-01-01'");
        let max = "340282366920938463463374607431768211455";
        assert_eq!(lit(ValueKind::Decimal, max), format!("'{max}'"));
        assert_eq!(lit(ValueKind::Decimal, "12345"), "12345");
        // Other databases keep a map as plain text.
        let pg = col("c", ValueKind::Map).literal(Dialect::Postgres, Some("{k: 1}"));
        assert_eq!(pg.unwrap(), "'{k: 1}'");
    }

    #[test]
    fn checks_numbers() {
        for ok in ["1", "-1", "+2.5", ".5", "5.", "1e10", "1.5E-3"] {
            assert!(is_number(ok), "{ok}");
        }
        for bad in ["", "-", ".", "1e", "1.2.3", "abc", "1 2", "0x10"] {
            assert!(!is_number(bad), "{bad}");
        }
        assert!(is_integer("-42"));
        assert!(!is_integer("4.2"));
        assert!(!is_integer("+"));
    }

    #[test]
    fn oracle_times_do_not_depend_on_nls() {
        let naive = col("t", ValueKind::Timestamp { zoned: false });
        let zoned = col("z", ValueKind::Timestamp { zoned: true });
        let ora = |c: &Column<'_>, v| c.literal(Dialect::Oracle, Some(v)).unwrap();
        assert_eq!(
            ora(&naive, "2021-02-03 04:05:06"),
            "TIMESTAMP '2021-02-03 04:05:06'"
        );
        assert_eq!(
            ora(&naive, "2024-02-29T13:45:59.123456789"),
            "TIMESTAMP '2024-02-29 13:45:59.123456789'"
        );
        assert_eq!(ora(&naive, "2024-02-29"), "DATE '2024-02-29'");
        assert_eq!(
            ora(&naive, "2024-02-29 10:30"),
            "TIMESTAMP '2024-02-29 10:30:00'"
        );
        assert_eq!(
            ora(&zoned, "2024-01-01 08:00:00.500+02:00"),
            "TIMESTAMP '2024-01-01 08:00:00.500 +02:00'"
        );
        assert_eq!(
            ora(&zoned, "2024-01-01T08:00:00Z"),
            "TIMESTAMP '2024-01-01 08:00:00 +00:00'"
        );
        // Anything else is left to the session's formats.
        assert_eq!(ora(&naive, "03-FEB-21"), "'03-FEB-21'");
        assert_eq!(
            ora(&naive, "0044-03-15 00:00:00 BC"),
            "'0044-03-15 00:00:00 BC'"
        );
        // Elsewhere a date stays a string.
        assert_eq!(ValueKind::of(&DataType::Date32), ValueKind::Date);
        assert_eq!(
            naive
                .literal(Dialect::Postgres, Some("2024-02-29"))
                .unwrap(),
            "'2024-02-29'"
        );
    }

    #[test]
    fn updates_with_typed_literals() {
        let cols = columns();
        let set = [
            (1, Value::Text("it's")),
            (2, Value::Text(" 12.50 ")),
            (3, Value::Text("yes")),
            (4, Value::Text("\\xDEAD")),
            (6, Value::Default),
        ];
        let key = [(0, Some("7"))];
        assert_eq!(
            update(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &set,
                &key
            )
            .unwrap(),
            "UPDATE demo.\"Items\" SET name = 'it''s', price = 12.50, active = TRUE, \
             data = decode('dead', 'hex'), \"order\" = DEFAULT WHERE id = 7"
        );
        assert_eq!(
            update(Dialect::Sqlite, &table(Dialect::Sqlite), &cols, &set, &key).unwrap(),
            "UPDATE demo.Items SET name = 'it''s', price = 12.50, active = 1, \
             data = X'dead', \"order\" = 0 WHERE id = 7"
        );
        assert_eq!(
            update(
                Dialect::MySql,
                &table(Dialect::MySql),
                &cols,
                &[(1, Value::Null)],
                &[(0, Some("1")), (1, None)]
            )
            .unwrap(),
            "UPDATE demo.Items SET name = NULL WHERE id = 1 AND name IS NULL"
        );
    }

    #[test]
    fn standard_sql_types_its_dates_and_times() {
        let lit = |kind, text| col("c", kind).literal(Dialect::Ansi, Some(text)).unwrap();
        assert_eq!(lit(ValueKind::Date, "2024-01-02"), "DATE '2024-01-02'");
        assert_eq!(lit(ValueKind::Time, "10:11:12"), "TIME '10:11:12'");
        assert_eq!(
            lit(
                ValueKind::Timestamp { zoned: false },
                "2024-01-02T03:04:05.5"
            ),
            "TIMESTAMP '2024-01-02 03:04:05.5'"
        );
        // Elsewhere the server converts the text.
        let pg = col("c", ValueKind::Date)
            .literal(Dialect::Postgres, Some("2024-01-02"))
            .unwrap();
        assert_eq!(pg, "'2024-01-02'");
        assert_eq!(ValueKind::of(&DataType::Date32), ValueKind::Date);
    }

    #[test]
    fn rejects_values_of_the_wrong_type() {
        let cols = columns();
        let err = update(
            Dialect::Postgres,
            &table(Dialect::Postgres),
            &cols,
            &[(0, Value::Text("x1"))],
            &[(0, Some("1"))],
        )
        .unwrap_err();
        assert_eq!(err.column, "id");
        assert!(err.message.contains("whole number"), "{err}");
        assert!(
            update(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &[(3, Value::Text("maybe"))],
                &[(0, Some("1"))]
            )
            .is_err()
        );
        assert!(
            update(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &[(4, Value::Text("abc"))],
                &[(0, Some("1"))]
            )
            .is_err()
        );
        assert!(
            update(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &[(1, Value::Text("x"))],
                &[]
            )
            .is_err(),
            "no key"
        );
        assert!(
            update(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &[(9, Value::Null)],
                &[(0, Some("1"))]
            )
            .is_err()
        );
    }

    #[test]
    fn writes_timestamps_for_each_dialect() {
        let cols = columns();
        let set = [(5, Value::Text("2024-03-01 10:00:00.5+00:00"))];
        let key = [(0, Some("1"))];
        assert!(
            update(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &set,
                &key
            )
            .unwrap()
            .contains("seen = '2024-03-01 10:00:00.5+00:00'")
        );
        assert!(
            update(Dialect::MySql, &table(Dialect::MySql), &cols, &set, &key)
                .unwrap()
                .contains("seen = '2024-03-01 10:00:00.5'")
        );
        // ClickHouse parses no offset in a literal: the instant goes through a function.
        let zoned = col("at", ValueKind::Timestamp { zoned: true });
        assert_eq!(
            zoned
                .literal(Dialect::ClickHouse, Some("2024-06-01T08:00:00Z"))
                .unwrap(),
            "parseDateTime64BestEffort('2024-06-01T08:00:00Z', 6)"
        );
        assert_eq!(
            zoned
                .literal(
                    Dialect::ClickHouse,
                    Some("2024-06-01T08:00:00.123456789+02:00")
                )
                .unwrap(),
            "parseDateTime64BestEffort('2024-06-01T08:00:00.123456789+02:00', 9)"
        );
    }

    #[test]
    fn writes_real_and_special_numeric_keys_for_postgres() {
        let real = col("r", ValueKind::Real);
        let key = [(0, Some("0.1"))];
        assert_eq!(
            where_clause(Dialect::Postgres, &[real], &key).unwrap(),
            " WHERE r = '0.1'::real"
        );
        assert_eq!(real.literal(Dialect::MySql, Some("0.1")).unwrap(), "0.1");
        assert!(real.literal(Dialect::Postgres, Some("x")).is_err());
        assert_eq!(
            ValueKind::of(&DataType::Float32),
            ValueKind::Real,
            "a float4"
        );
        let numeric = col("n", ValueKind::Decimal);
        assert_eq!(
            numeric.literal(Dialect::Postgres, Some("NaN")).unwrap(),
            "'NaN'::numeric"
        );
        assert_eq!(
            numeric
                .literal(Dialect::Postgres, Some("-Infinity"))
                .unwrap(),
            "'-Infinity'::numeric"
        );
        assert!(numeric.literal(Dialect::MySql, Some("NaN")).is_err());
    }

    #[test]
    fn inserts_and_deletes() {
        let cols = columns();
        assert_eq!(
            insert(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &[(0, Value::Default), (1, Value::Text("a")), (2, Value::Null)]
            )
            .unwrap(),
            "INSERT INTO demo.\"Items\" (name, price) VALUES ('a', NULL)"
        );
        assert_eq!(
            insert(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &[(0, Value::Default)]
            )
            .unwrap(),
            "INSERT INTO demo.\"Items\" DEFAULT VALUES"
        );
        assert_eq!(
            insert(Dialect::MySql, "t", &cols, &[]).unwrap(),
            "INSERT INTO t () VALUES ()"
        );
        assert_eq!(
            delete(
                Dialect::MySql,
                &table(Dialect::MySql),
                &cols,
                &[(0, Some("3")), (1, Some("a`b"))]
            )
            .unwrap(),
            "DELETE FROM demo.Items WHERE id = 3 AND name = 'a`b'"
        );
    }

    fn sql(statements: &[Statement]) -> Vec<(&str, u64)> {
        statements
            .iter()
            .map(|s| (s.sql.as_str(), s.rows))
            .collect()
    }

    #[test]
    fn deletes_rows_by_lists_of_keys() {
        let cols = columns();
        let pg = Dialect::Postgres;
        let keys: Vec<Vec<(usize, Option<&str>)>> = ["1", "2", "3"]
            .iter()
            .map(|id| vec![(0, Some(*id))])
            .collect();
        assert_eq!(
            sql(&deletes(pg, &table(pg), &cols, &keys).unwrap()),
            [("DELETE FROM demo.\"Items\" WHERE id IN (1, 2, 3)", 3)]
        );
        // A lone row keeps the plain form.
        assert_eq!(
            sql(&deletes(pg, &table(pg), &cols, &keys[..1]).unwrap()),
            [("DELETE FROM demo.\"Items\" WHERE id = 1", 1)]
        );
        assert_eq!(deletes(pg, "t", &cols, &[]).unwrap(), Vec::new());
        // Text keys are quoted per dialect, and checked against the column type.
        let names = [vec![(1, Some("it's"))], vec![(1, Some("a\\b"))]];
        assert_eq!(
            sql(&deletes(Dialect::MySql, "t", &cols, &names).unwrap()),
            [(
                "DELETE FROM t WHERE name IN ('it''s', _utf8mb4 X'615c62')",
                2
            )]
        );
        assert_eq!(
            sql(&deletes(Dialect::SqlServer, "t", &cols, &names).unwrap()),
            [("DELETE FROM t WHERE name IN (N'it''s', N'a\\b')", 2)]
        );
        let bad = [vec![(0, Some("1"))], vec![(0, Some("x"))]];
        assert!(deletes(pg, "t", &cols, &bad).is_err());
    }

    #[test]
    fn deletes_rows_by_composite_keys_per_dialect() {
        let cols = columns();
        let keys = [
            vec![(0, Some("1")), (1, Some("a"))],
            vec![(0, Some("2")), (1, Some("b"))],
        ];
        assert_eq!(
            sql(&deletes(Dialect::Postgres, "t", &cols, &keys).unwrap()),
            [("DELETE FROM t WHERE (id, name) IN ((1, 'a'), (2, 'b'))", 2)]
        );
        assert_eq!(
            sql(&deletes(Dialect::Sqlite, "t", &cols, &keys).unwrap()),
            [(
                "DELETE FROM t WHERE (id, name) IN (VALUES (1, 'a'), (2, 'b'))",
                2
            )]
        );
        for d in [
            Dialect::MySql,
            Dialect::ClickHouse,
            Dialect::BigQuery,
            Dialect::DuckDb,
        ] {
            assert_eq!(
                sql(&deletes(d, "t", &cols, &keys).unwrap()),
                [(
                    "DELETE FROM t WHERE (id = 1 AND name = 'a') OR (id = 2 AND name = 'b')",
                    2
                )],
                "{d:?}"
            );
        }
        assert_eq!(
            deletes(Dialect::SqlServer, "t", &cols, &keys).unwrap()[0].sql,
            "DELETE FROM t WHERE (id = 1 AND name = N'a') OR (id = 2 AND name = N'b')"
        );
        assert_eq!(
            deletes(Dialect::Oracle, "t", &cols, &keys).unwrap()[0].sql,
            r#"DELETE FROM t WHERE ("id" = 1 AND "name" = 'a') OR ("id" = 2 AND "name" = 'b')"#
        );
    }

    #[test]
    fn deletes_keys_with_null_one_by_one() {
        let cols = columns();
        let keys = [
            vec![(0, Some("1")), (1, Some("a"))],
            vec![(0, Some("2")), (1, None)],
            vec![(0, Some("3")), (1, Some("c"))],
        ];
        assert_eq!(
            sql(&deletes(Dialect::Postgres, "t", &cols, &keys).unwrap()),
            [
                ("DELETE FROM t WHERE (id, name) IN ((1, 'a'), (3, 'c'))", 2),
                ("DELETE FROM t WHERE id = 2 AND name IS NULL", 1),
            ]
        );
    }

    #[test]
    fn deletes_in_batches() {
        let cols = columns();
        let ids: Vec<String> = (1..=BATCH_ROWS * 2 + 1).map(|i| i.to_string()).collect();
        let keys: Vec<_> = ids.iter().map(|id| vec![(0, Some(id.as_str()))]).collect();
        let statements = deletes(Dialect::Oracle, "t", &cols, &keys).unwrap();
        assert_eq!(
            statements.iter().map(|s| s.rows).collect::<Vec<_>>(),
            [500, 500, 1]
        );
        assert!(
            statements[0]
                .sql
                .starts_with(r#"DELETE FROM t WHERE "id" IN (1, 2, "#)
        );
        assert!(statements[0].sql.ends_with(", 499, 500)"));
        assert!(
            statements[1]
                .sql
                .starts_with(r#"DELETE FROM t WHERE "id" IN (501, "#)
        );
        assert_eq!(statements[2].sql, r#"DELETE FROM t WHERE "id" = 1001"#);
    }

    #[test]
    fn inserts_rows_in_batches_of_the_same_columns() {
        let cols = columns();
        let pg = Dialect::Postgres;
        let row = |id: &'static str, name: Value<'static>| vec![(0, Value::Text(id)), (1, name)];
        let rows = [
            row("1", Value::Text("a")),
            row("2", Value::Null),
            // Left at its default: another column list.
            row("3", Value::Default),
            vec![(1, Value::Default)],
            row("4", Value::Text("it's")),
        ];
        assert_eq!(
            sql(&inserts(pg, "t", &cols, &rows).unwrap()),
            [
                ("INSERT INTO t (id, name) VALUES (1, 'a'), (2, NULL)", 2),
                ("INSERT INTO t (id) VALUES (3)", 1),
                ("INSERT INTO t DEFAULT VALUES", 1),
                ("INSERT INTO t (id, name) VALUES (4, 'it''s')", 1),
            ]
        );
        // One row per statement where `VALUES` takes one row.
        assert_eq!(
            sql(&inserts(Dialect::Oracle, "t", &cols, &rows[..2]).unwrap()),
            [
                (r#"INSERT INTO t ("id", "name") VALUES (1, 'a')"#, 1),
                (r#"INSERT INTO t ("id", "name") VALUES (2, NULL)"#, 1),
            ]
        );
        assert!(inserts(pg, "t", &cols, &[vec![(0, Value::Text("x"))]]).is_err());
    }

    #[test]
    fn inserts_many_rows_in_chunks() {
        let cols = columns();
        let ids: Vec<String> = (1..=BATCH_ROWS + 2).map(|i| i.to_string()).collect();
        let rows: Vec<_> = ids.iter().map(|id| vec![(0, Value::Text(id))]).collect();
        let statements = inserts(Dialect::SqlServer, "t", &cols, &rows).unwrap();
        assert_eq!(
            statements.iter().map(|s| s.rows).collect::<Vec<_>>(),
            [500, 2]
        );
        assert!(statements[0].sql.ends_with("(499), (500)"));
        assert_eq!(statements[1].sql, "INSERT INTO t (id) VALUES (501), (502)");
    }

    #[test]
    fn saves_behind_a_savepoint_where_there_is_one() {
        let s = |v: &[&str]| v.iter().map(|&x| x.to_owned()).collect::<Vec<_>>();
        assert_eq!(
            bracket(Dialect::Oracle, false),
            (Some("BEGIN".to_owned()), s(&["COMMIT"]), s(&["ROLLBACK"]))
        );
        assert_eq!(
            bracket(Dialect::Postgres, true),
            (
                Some("SAVEPOINT platypus_edit".to_owned()),
                s(&["RELEASE SAVEPOINT platypus_edit"]),
                s(&[
                    "ROLLBACK TO SAVEPOINT platypus_edit",
                    "RELEASE SAVEPOINT platypus_edit"
                ])
            )
        );
        // Oracle has no RELEASE: the savepoint lasts until the transaction ends.
        assert_eq!(
            bracket(Dialect::Oracle, true),
            (
                Some("SAVEPOINT platypus_edit".to_owned()),
                Vec::new(),
                s(&["ROLLBACK TO SAVEPOINT platypus_edit"])
            )
        );
        assert_eq!(
            bracket(Dialect::DuckDb, true),
            (None, Vec::new(), s(&["ROLLBACK"]))
        );
        let rolled_back = ApplyError::SessionRolledBack(Box::new(ApplyError::RowCount {
            index: 0,
            rows: 0,
            expected: 1,
        }));
        assert!(
            rolled_back
                .to_string()
                .contains("the whole transaction was rolled back"),
            "{rolled_back}"
        );
    }

    #[test]
    fn writes_decimals_without_exponent() {
        for (typed, plain) in [
            ("1e3", "1000"),
            ("-1.5E-3", "-0.0015"),
            ("+2.50e1", "25.0"),
            (".5e1", "5"),
            ("5.e2", "500"),
            ("0.001e2", "0.1"),
            ("12.50", "12.50"),
            ("-7", "-7"),
        ] {
            assert_eq!(plain_number(typed), plain, "{typed}");
        }
        // Too far to write out: left as typed, for the server to refuse.
        assert_eq!(plain_number("1e99999"), "1e99999");
        let decimal = col("n", ValueKind::Decimal);
        let float = col("f", ValueKind::Float);
        let pg = Dialect::Postgres;
        assert_eq!(decimal.literal(pg, Some(" 1.25e2 ")).unwrap(), "125");
        assert_eq!(float.literal(pg, Some("1.25e2")).unwrap(), "1.25e2");
    }

    #[test]
    fn quotes_backslashes_whatever_the_server_reads() {
        let cols = columns();
        let pg = Dialect::Postgres;
        let sql = insert(
            pg,
            "t",
            &cols,
            &[(1, Value::Text("x\\'); DROP TABLE t; --"))],
        )
        .unwrap();
        assert_eq!(
            sql,
            "INSERT INTO t (name) VALUES (E'x\\\\''); DROP TABLE t; --')"
        );
        let keys = [vec![(1, Some("a\\")), (0, Some("1"))]];
        assert_eq!(
            deletes(pg, "t", &cols, &keys).unwrap()[0].sql,
            "DELETE FROM t WHERE name = E'a\\\\' AND id = 1"
        );
    }

    #[test]
    fn batches_stay_under_a_byte_budget() {
        let cols = columns();
        let long: Vec<String> = (0..5)
            .map(|i| format!("{i}{}", "x".repeat(400_000)))
            .collect();
        let keys: Vec<_> = long.iter().map(|v| vec![(1, Some(v.as_str()))]).collect();
        let statements = deletes(Dialect::MySql, "t", &cols, &keys).unwrap();
        assert_eq!(
            statements.iter().map(|s| s.rows).collect::<Vec<_>>(),
            [2, 2, 1]
        );
        assert!(statements.iter().all(|s| s.sql.len() <= BATCH_BYTES + 100));
        assert!(
            statements[2]
                .sql
                .starts_with("DELETE FROM t WHERE name = '4x")
        );

        let rows: Vec<_> = long.iter().map(|v| vec![(1, Value::Text(v))]).collect();
        let statements = inserts(Dialect::MySql, "t", &cols, &rows).unwrap();
        assert_eq!(
            statements.iter().map(|s| s.rows).collect::<Vec<_>>(),
            [2, 2, 1]
        );
        assert!(statements.iter().all(|s| s.sql.len() <= BATCH_BYTES + 100));
    }

    #[test]
    fn keeps_numbers_and_strings_apart_in_values_lists() {
        let cols = [col("id", ValueKind::Integer), col("f", ValueKind::Float)];
        let row = |id, f| vec![(0, Value::Text(id)), (1, f)];
        let rows = [
            row("1", Value::Text("1.5")),
            row("2", Value::Null),
            row("3", Value::Text("2")),
            row("4", Value::Text("NaN")),
            row("5", Value::Null),
            row("6", Value::Text("infinity")),
            row("7", Value::Text("1e3")),
        ];
        assert_eq!(
            sql(&inserts(Dialect::SqlServer, "t", &cols, &rows).unwrap()),
            [
                (
                    "INSERT INTO t (id, f) VALUES (1, 1.5), (2, NULL), (3, 2)",
                    3
                ),
                (
                    "INSERT INTO t (id, f) VALUES (4, N'NaN'), (5, NULL), (6, N'infinity')",
                    3
                ),
                ("INSERT INTO t (id, f) VALUES (7, 1e3)", 1),
            ]
        );
    }

    #[test]
    fn inserts_one_row_per_statement_for_generic_drivers() {
        let cols = columns();
        let rows = [vec![(0, Value::Text("1"))], vec![(0, Value::Text("2"))]];
        assert_eq!(
            sql(&inserts(Dialect::Ansi, "t", &cols, &rows).unwrap()),
            [
                ("INSERT INTO t (\"id\") VALUES (1)", 1),
                ("INSERT INTO t (\"id\") VALUES (2)", 1),
            ]
        );
    }

    #[test]
    fn restores_rows_with_their_identity_values() {
        let cols = columns();
        let rows = [
            vec![(0, Value::Text("1")), (1, Value::Text("a"))],
            vec![(0, Value::Text("2")), (1, Value::Null)],
        ];
        let pg = table(Dialect::Postgres);
        assert_eq!(
            restore_inserts(Dialect::Postgres, &pg, &cols, &rows, &[0]).unwrap(),
            [
                "INSERT INTO demo.\"Items\" (id, name) OVERRIDING SYSTEM VALUE VALUES (1, 'a'), (2, NULL)"
            ]
        );
        // Serials and the like take the values as they are.
        assert_eq!(
            restore_inserts(Dialect::Postgres, &pg, &cols, &rows, &[]).unwrap(),
            ["INSERT INTO demo.\"Items\" (id, name) VALUES (1, 'a'), (2, NULL)"]
        );
        let mssql = table(Dialect::SqlServer);
        assert_eq!(
            restore_inserts(Dialect::SqlServer, &mssql, &cols, &rows, &[0]).unwrap(),
            [
                "SET IDENTITY_INSERT demo.Items ON",
                "INSERT INTO demo.Items (id, name) VALUES (1, N'a'), (2, NULL)",
                "SET IDENTITY_INSERT demo.Items OFF",
            ]
        );
        // No identity value given: nothing to switch on.
        assert_eq!(
            restore_inserts(
                Dialect::SqlServer,
                &mssql,
                &cols,
                &[vec![(1, Value::Text("a"))]],
                &[0]
            )
            .unwrap(),
            ["INSERT INTO demo.Items (name) VALUES (N'a')"]
        );
        // Rows that set the identity and rows that do not go in statements of their own.
        assert_eq!(
            restore_inserts(
                Dialect::Postgres,
                &pg,
                &cols,
                &[vec![(1, Value::Text("a"))], rows[0].clone()],
                &[0]
            )
            .unwrap(),
            [
                "INSERT INTO demo.\"Items\" (name) VALUES ('a')",
                "INSERT INTO demo.\"Items\" (id, name) OVERRIDING SYSTEM VALUE VALUES (1, 'a')",
            ]
        );
    }

    #[test]
    fn explains_row_counts() {
        let count = |rows, expected| {
            ApplyError::RowCount {
                index: 1,
                rows,
                expected,
            }
            .to_string()
        };
        assert!(count(0, 1).contains("the row was not found"));
        assert!(count(2, 1).contains("2 rows instead of one"));
        assert!(count(498, 500).contains("only 498 of its 500 rows"));
        assert!(count(501, 500).contains("501 rows instead of 500"));
    }
}
