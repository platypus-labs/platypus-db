//! Decoders for the PostgreSQL binary wire format.
//!
//! Scalar types with a native Arrow equivalent are decoded straight into Arrow values;
//! everything else is rendered as text by [`format_value`], close to PostgreSQL's own output.

use std::error::Error as StdError;
use std::fmt::Write as _;
use std::net::IpAddr;

use fallible_iterator::FallibleIterator;
use postgres_protocol::types as pg;
use tokio_postgres::types::{Kind, Type};

pub type DecodeResult<T> = Result<T, Box<dyn StdError + Sync + Send>>;

/// Days between 1970-01-01 (Arrow epoch) and 2000-01-01 (PostgreSQL epoch).
pub const PG_EPOCH_DAYS: i32 = 10_957;
/// Microseconds between 1970-01-01 and 2000-01-01.
pub const PG_EPOCH_MICROS: i64 = 946_684_800_000_000;

const MICROS_PER_SECOND: i64 = 1_000_000;

fn invalid(what: &str) -> Box<dyn StdError + Sync + Send> {
    format!("invalid {what} value").into()
}

fn take<const N: usize>(buf: &[u8], what: &str) -> DecodeResult<[u8; N]> {
    buf.get(..N)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| invalid(what))
}

/// Decoded `numeric`, base-10000 digits as sent on the wire.
#[derive(Debug, PartialEq, Eq)]
pub enum Numeric {
    NaN,
    PosInfinity,
    NegInfinity,
    Finite {
        negative: bool,
        /// Base-10000 exponent of the first digit.
        weight: i16,
        /// Number of decimal digits after the point.
        dscale: u16,
        digits: Vec<i16>,
    },
}

pub fn numeric_from_sql(buf: &[u8]) -> DecodeResult<Numeric> {
    let header: [u8; 8] = take(buf, "numeric")?;
    let ndigits = usize::try_from(i16::from_be_bytes([header[0], header[1]]))
        .map_err(|_| invalid("numeric"))?;
    let weight = i16::from_be_bytes([header[2], header[3]]);
    let sign = u16::from_be_bytes([header[4], header[5]]);
    let dscale = u16::from_be_bytes([header[6], header[7]]);
    let body = &buf[8..];
    if body.len() != ndigits * 2 {
        return Err(invalid("numeric"));
    }
    let digits = body
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_be_bytes(*c))
        .collect();
    Ok(match sign {
        0x0000 => Numeric::Finite {
            negative: false,
            weight,
            dscale,
            digits,
        },
        0x4000 => Numeric::Finite {
            negative: true,
            weight,
            dscale,
            digits,
        },
        0xC000 => Numeric::NaN,
        0xD000 => Numeric::PosInfinity,
        0xF000 => Numeric::NegInfinity,
        _ => return Err(invalid("numeric")),
    })
}

impl Numeric {
    /// Unscaled `i128` for an Arrow `Decimal128` with the given scale, or `None` when the
    /// value is not finite or does not fit.
    #[must_use]
    pub fn to_i128(&self, scale: i8) -> Option<i128> {
        let Self::Finite {
            negative,
            weight,
            digits,
            ..
        } = self
        else {
            return None;
        };
        let mut acc: i128 = 0;
        for (i, &digit) in digits.iter().enumerate() {
            let exp = 4 * (i32::from(*weight) - i32::try_from(i).ok()?) + i32::from(scale);
            let term = if exp >= 0 {
                i128::from(digit).checked_mul(10i128.checked_pow(exp.unsigned_abs())?)?
            } else {
                // Only reachable for digits beyond the column scale, which PostgreSQL rounds away.
                i128::from(digit) / 10i128.checked_pow(exp.unsigned_abs())?
            };
            acc = acc.checked_add(term)?;
        }
        Some(if *negative { -acc } else { acc })
    }
}

impl std::fmt::Display for Numeric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (negative, weight, dscale, digits) = match self {
            Self::NaN => return f.write_str("NaN"),
            Self::PosInfinity => return f.write_str("Infinity"),
            Self::NegInfinity => return f.write_str("-Infinity"),
            Self::Finite {
                negative,
                weight,
                dscale,
                digits,
            } => (*negative, i32::from(*weight), usize::from(*dscale), digits),
        };
        let digit_at = |pos: i32| -> i16 {
            // `pos` is the base-10000 exponent; digit index is weight - pos.
            usize::try_from(weight - pos)
                .ok()
                .and_then(|i| digits.get(i).copied())
                .unwrap_or(0)
        };
        let mut out = String::new();
        if negative && digits.iter().any(|&d| d != 0) {
            out.push('-');
        }
        if weight < 0 {
            out.push('0');
        } else {
            let _ = write!(out, "{}", digit_at(weight));
            for pos in (0..weight).rev() {
                let _ = write!(out, "{:04}", digit_at(pos));
            }
        }
        if dscale > 0 {
            out.push('.');
            let mut frac = String::with_capacity(dscale + 4);
            let mut pos = -1;
            while frac.len() < dscale {
                let _ = write!(frac, "{:04}", digit_at(pos));
                pos -= 1;
            }
            out.push_str(&frac[..dscale]);
        }
        f.write_str(&out)
    }
}

/// Splits a PostgreSQL `numeric` type modifier into `(precision, scale)`.
#[must_use]
pub fn numeric_typmod(typmod: i32) -> Option<(u8, i8)> {
    if typmod < 4 {
        return None;
    }
    let raw = typmod - 4;
    let precision = u8::try_from((raw >> 16) & 0xffff).ok()?;
    // Scale is an 11-bit signed field since PostgreSQL 15.
    let scale = i8::try_from(((raw & 0x7ff) ^ 0x400) - 0x400).ok()?;
    Some((precision, scale))
}

/// Days since the Unix epoch to `(year, month, day)` in the proleptic Gregorian calendar.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn write_date(out: &mut String, unix_days: i64) {
    let (year, month, day) = civil_from_days(unix_days);
    if year <= 0 {
        let _ = write!(out, "{:04}-{month:02}-{day:02} BC", 1 - year);
    } else {
        let _ = write!(out, "{year:04}-{month:02}-{day:02}");
    }
}

fn write_time(out: &mut String, micros: i64) {
    let secs = micros.div_euclid(MICROS_PER_SECOND);
    let frac = micros.rem_euclid(MICROS_PER_SECOND);
    let _ = write!(
        out,
        "{:02}:{:02}:{:02}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    );
    if frac != 0 {
        let s = format!("{frac:06}");
        out.push('.');
        out.push_str(s.trim_end_matches('0'));
    }
}

fn write_offset(out: &mut String, seconds_east: i32) {
    let sign = if seconds_east < 0 { '-' } else { '+' };
    let abs = seconds_east.unsigned_abs();
    let _ = write!(out, "{sign}{:02}", abs / 3600);
    if !abs.is_multiple_of(3600) {
        let _ = write!(out, ":{:02}", abs / 60 % 60);
    }
}

#[must_use]
pub fn format_date(pg_days: i32) -> String {
    match pg_days {
        i32::MAX => "infinity".to_owned(),
        i32::MIN => "-infinity".to_owned(),
        d => {
            let mut out = String::with_capacity(10);
            write_date(&mut out, i64::from(d) + i64::from(PG_EPOCH_DAYS));
            out
        }
    }
}

#[must_use]
pub fn format_timestamp(pg_micros: i64, utc: bool) -> String {
    match pg_micros {
        i64::MAX => return "infinity".to_owned(),
        i64::MIN => return "-infinity".to_owned(),
        _ => {}
    }
    let unix_micros = i128::from(pg_micros) + i128::from(PG_EPOCH_MICROS);
    let micros_per_day = i128::from(86_400 * MICROS_PER_SECOND);
    let days = i64::try_from(unix_micros.div_euclid(micros_per_day)).unwrap_or_default();
    let time = i64::try_from(unix_micros.rem_euclid(micros_per_day)).unwrap_or_default();
    let mut out = String::with_capacity(32);
    write_date(&mut out, days);
    // Keep the BC suffix at the end, like PostgreSQL.
    let bc = out.ends_with(" BC");
    if bc {
        out.truncate(out.len() - 3);
    }
    out.push(' ');
    write_time(&mut out, time);
    if utc {
        out.push_str("+00");
    }
    if bc {
        out.push_str(" BC");
    }
    out
}

#[must_use]
pub fn format_interval(micros: i64, days: i32, months: i32) -> String {
    let mut parts: Vec<String> = Vec::new();
    let plural = |n: i64, unit: &str| {
        if n.abs() == 1 {
            format!("{n} {unit}")
        } else {
            format!("{n} {unit}s")
        }
    };
    let (years, mons) = (months / 12, months % 12);
    if years != 0 {
        parts.push(plural(years.into(), "year"));
    }
    if mons != 0 {
        parts.push(plural(mons.into(), "mon"));
    }
    if days != 0 {
        parts.push(plural(days.into(), "day"));
    }
    if micros != 0 || parts.is_empty() {
        let mut time = String::new();
        if micros < 0 {
            time.push('-');
        }
        write_time(&mut time, micros.checked_abs().unwrap_or(i64::MAX));
        parts.push(time);
    }
    parts.join(" ")
}

pub fn interval_from_sql(buf: &[u8]) -> DecodeResult<(i64, i32, i32)> {
    let b: [u8; 16] = take(buf, "interval")?;
    let micros = i64::from_be_bytes(b[0..8].try_into()?);
    let days = i32::from_be_bytes(b[8..12].try_into()?);
    let months = i32::from_be_bytes(b[12..16].try_into()?);
    Ok((micros, days, months))
}

fn hex(out: &mut String, bytes: &[u8]) {
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
}

fn format_bytea(raw: &[u8]) -> String {
    let mut out = String::with_capacity(2 + raw.len() * 2);
    out.push_str("\\x");
    hex(&mut out, raw);
    out
}

fn format_uuid(raw: &[u8]) -> DecodeResult<String> {
    let b = pg::uuid_from_sql(raw)?;
    let mut out = String::with_capacity(36);
    for (i, chunk) in [&b[0..4], &b[4..6], &b[6..8], &b[8..10], &b[10..16]]
        .iter()
        .enumerate()
    {
        if i > 0 {
            out.push('-');
        }
        hex(&mut out, chunk);
    }
    Ok(out)
}

fn format_inet(raw: &[u8], always_mask: bool) -> DecodeResult<String> {
    let inet = pg::inet_from_sql(raw)?;
    let full = match inet.addr() {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    };
    Ok(if always_mask || inet.netmask() != full {
        format!("{}/{}", inet.addr(), inet.netmask())
    } else {
        inet.addr().to_string()
    })
}

/// Where an element or field is written: PostgreSQL quotes different characters in each.
#[derive(Clone, Copy)]
enum Literal {
    /// An array element, followed by the element type's delimiter.
    Array(char),
    Composite,
}

/// Whether `s` is quoted inside a literal, as `array_out` and `record_out` decide.
fn needs_quotes(s: &str, literal: Literal) -> bool {
    match literal {
        Literal::Array(delimiter) => {
            s.is_empty()
                || s.eq_ignore_ascii_case("null")
                || s.chars()
                    .any(|c| c == delimiter || matches!(c, '"' | '\\' | '{' | '}') || pg_space(c))
        }
        Literal::Composite => {
            s.is_empty()
                || s.chars()
                    .any(|c| matches!(c, '"' | '\\' | '(' | ')' | ',') || pg_space(c))
        }
    }
}

/// The white space PostgreSQL quotes (`array_isspace`, `isspace` in `record_out`): ASCII.
fn pg_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0B' | '\x0C')
}

/// `value` as an element or field, quoted when needed. Inside quotes, `array_out` escapes
/// `"` and `\` with a backslash, `record_out` doubles them.
fn push_element(out: &mut String, value: &str, literal: Literal) {
    if needs_quotes(value, literal) {
        out.push('"');
        for c in value.chars() {
            if matches!(c, '"' | '\\') {
                out.push(match literal {
                    Literal::Array(_) => '\\',
                    Literal::Composite => c,
                });
            }
            out.push(c);
        }
        out.push('"');
    } else {
        out.push_str(value);
    }
}

/// The character between array elements: `;` for boxes, whose text holds commas.
fn array_delimiter(element: &Type) -> char {
    match element.kind() {
        Kind::Domain(base) => array_delimiter(base),
        _ if *element == Type::BOX => ';',
        _ => ',',
    }
}

/// An array as `array_out` prints it: `{1,2}`, `{{1,2},{3,4}}`, and with the bounds first
/// when one of them does not start at 1, e.g. `[0:1]={1,2}`, so that saving the text back
/// keeps them.
fn format_array(element: &Type, raw: &[u8]) -> DecodeResult<String> {
    let array = pg::array_from_sql(raw)?;
    let bounds: Vec<(i32, usize)> = array
        .dimensions()
        .map(|d| Ok((d.lower_bound, usize::try_from(d.len).unwrap_or(0))))
        .collect()?;
    let mut values = array.values();
    if bounds.is_empty() {
        return Ok("{}".to_owned());
    }
    let mut out = String::new();
    if bounds.iter().any(|&(lower, _)| lower != 1) {
        for &(lower, len) in &bounds {
            let upper = i64::from(lower) + i64::try_from(len)? - 1;
            let _ = write!(out, "[{lower}:{upper}]");
        }
        out.push('=');
    }
    let dims: Vec<usize> = bounds.iter().map(|&(_, len)| len).collect();
    let delimiter = array_delimiter(element);
    write_array_level(&mut out, element, delimiter, &dims, &mut values)?;
    Ok(out)
}

fn write_array_level(
    out: &mut String,
    element: &Type,
    delimiter: char,
    dims: &[usize],
    values: &mut pg::ArrayValues<'_>,
) -> DecodeResult<()> {
    out.push('{');
    for i in 0..dims[0] {
        if i > 0 {
            out.push(delimiter);
        }
        if dims.len() > 1 {
            write_array_level(out, element, delimiter, &dims[1..], values)?;
        } else {
            match values.next()?.ok_or_else(|| invalid("array"))? {
                None => out.push_str("NULL"),
                Some(raw) => {
                    push_element(out, &format_value(element, raw)?, Literal::Array(delimiter));
                }
            }
        }
    }
    out.push('}');
    Ok(())
}

fn format_range(element: &Type, raw: &[u8]) -> DecodeResult<String> {
    use pg::{Range, RangeBound};
    let (lower, upper) = match pg::range_from_sql(raw)? {
        Range::Empty => return Ok("empty".to_owned()),
        Range::Nonempty(lower, upper) => (lower, upper),
    };
    let bound = |b: Option<&[u8]>| -> DecodeResult<String> {
        b.map_or_else(|| Ok(String::new()), |raw| format_value(element, raw))
    };
    let mut out = String::new();
    match lower {
        RangeBound::Inclusive(v) => {
            out.push('[');
            out.push_str(&bound(v)?);
        }
        RangeBound::Exclusive(v) => {
            out.push('(');
            out.push_str(&bound(v)?);
        }
        RangeBound::Unbounded => out.push('('),
    }
    out.push(',');
    match upper {
        RangeBound::Inclusive(v) => {
            out.push_str(&bound(v)?);
            out.push(']');
        }
        RangeBound::Exclusive(v) => {
            out.push_str(&bound(v)?);
            out.push(')');
        }
        RangeBound::Unbounded => out.push(')'),
    }
    Ok(out)
}

fn format_composite(
    fields: &[tokio_postgres::types::Field],
    mut buf: &[u8],
) -> DecodeResult<String> {
    let count = i32::from_be_bytes(take(buf, "record")?);
    buf = &buf[4..];
    let mut out = String::from("(");
    for i in 0..usize::try_from(count).map_err(|_| invalid("record"))? {
        if i > 0 {
            out.push(',');
        }
        let header: [u8; 8] = take(buf, "record")?;
        let len = i32::from_be_bytes([header[4], header[5], header[6], header[7]]);
        buf = &buf[8..];
        let Ok(len) = usize::try_from(len) else {
            continue; // NULL field: empty between delimiters
        };
        let value = buf.get(..len).ok_or_else(|| invalid("record"))?;
        buf = &buf[len..];
        // An anonymous `record` names each field's type only in the field's header.
        let oid = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let named = Type::from_oid(oid);
        let ty = fields
            .get(i)
            .map(tokio_postgres::types::Field::type_)
            .or(named.as_ref())
            .unwrap_or(&Type::TEXT);
        push_element(&mut out, &format_value(ty, value)?, Literal::Composite);
    }
    out.push(')');
    Ok(out)
}

/// The `f64`s at the start of `buf`, big-endian.
fn floats<const N: usize>(buf: &[u8], what: &str) -> DecodeResult<[f64; N]> {
    let mut out = [0.0; N];
    for (i, v) in out.iter_mut().enumerate() {
        let bytes = buf.get(i * 8..i * 8 + 8).ok_or_else(|| invalid(what))?;
        *v = f64::from_be_bytes(bytes.try_into()?);
    }
    Ok(out)
}

/// `(x,y)` for each of `count` points at the start of `buf`, comma separated.
fn write_points(out: &mut String, buf: &[u8], count: usize, what: &str) -> DecodeResult<()> {
    for i in 0..count {
        if i > 0 {
            out.push(',');
        }
        let [x, y] = floats::<2>(buf.get(i * 16..).ok_or_else(|| invalid(what))?, what)?;
        let _ = write!(out, "({x},{y})");
    }
    Ok(())
}

/// The geometric types other than `point`, as PostgreSQL prints them.
fn format_geometric(ty: &Type, raw: &[u8]) -> DecodeResult<String> {
    let mut out = String::new();
    match *ty {
        Type::BOX => write_points(&mut out, raw, 2, "box")?,
        Type::LSEG => {
            out.push('[');
            write_points(&mut out, raw, 2, "lseg")?;
            out.push(']');
        }
        Type::PATH | Type::POLYGON => {
            // A path starts with a closed flag; both then have the number of points.
            let (closed, rest) = if *ty == Type::PATH {
                let (&flag, rest) = raw.split_first().ok_or_else(|| invalid("path"))?;
                (flag != 0, rest)
            } else {
                (true, raw)
            };
            let count = i32::from_be_bytes(take(rest, "path")?);
            let count = usize::try_from(count).map_err(|_| invalid("path"))?;
            out.push(if closed { '(' } else { '[' });
            write_points(&mut out, &rest[4..], count, "path")?;
            out.push(if closed { ')' } else { ']' });
        }
        Type::LINE => {
            let [a, b, c] = floats::<3>(raw, "line")?;
            let _ = write!(out, "{{{a},{b},{c}}}");
        }
        _ => {
            let [x, y, r] = floats::<3>(raw, "circle")?;
            let _ = write!(out, "<({x},{y}),{r}>");
        }
    }
    Ok(out)
}

/// `bit(n)` and `bit varying`: the number of bits, then the bits, first bit highest.
fn format_bits(raw: &[u8]) -> DecodeResult<String> {
    let len = usize::try_from(i32::from_be_bytes(take(raw, "bit")?))?;
    let bytes = &raw[4..];
    (0..len)
        .map(|i| {
            let byte = bytes.get(i / 8)?;
            Some(if (byte >> (7 - i % 8)) & 1 == 1 {
                '1'
            } else {
                '0'
            })
        })
        .collect::<Option<String>>()
        .ok_or_else(|| invalid("bit"))
}

/// A NUL-terminated string at the start of `buf`, and what follows it.
fn c_string<'a>(buf: &'a [u8], what: &str) -> DecodeResult<(&'a str, &'a [u8])> {
    let end = buf
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| invalid(what))?;
    Ok((std::str::from_utf8(&buf[..end])?, &buf[end + 1..]))
}

/// A lexeme or operand quoted as `tsvectorout` and `tsqueryout` do: `'` and `\` doubled.
fn push_quoted_lexeme(out: &mut String, lexeme: &str) {
    out.push('\'');
    for c in lexeme.chars() {
        if matches!(c, '\'' | '\\') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
}

/// `tsvector`: the lexemes, each with its positions and their weights (`'fox':3A,5`).
fn format_tsvector(raw: &[u8]) -> DecodeResult<String> {
    let count = i32::from_be_bytes(take(raw, "tsvector")?);
    let mut buf = &raw[4..];
    let mut out = String::new();
    for i in 0..count {
        if i > 0 {
            out.push(' ');
        }
        let (lexeme, rest) = c_string(buf, "tsvector")?;
        push_quoted_lexeme(&mut out, lexeme);
        let positions = u16::from_be_bytes(take(rest, "tsvector")?);
        buf = &rest[2..];
        for p in 0..usize::from(positions) {
            let entry = u16::from_be_bytes(take(buf.get(p * 2..).unwrap_or_default(), "tsvector")?);
            out.push(if p == 0 { ':' } else { ',' });
            let _ = write!(out, "{}", entry & 0x3fff);
            match entry >> 14 {
                3 => out.push('A'),
                2 => out.push('B'),
                1 => out.push('C'),
                _ => {}
            }
        }
        buf = &buf[usize::from(positions) * 2..];
    }
    Ok(out)
}

/// One item of a `tsquery` in its wire order: an operator, then its right operand, then
/// its left one.
enum QueryItem<'a> {
    Operand {
        text: &'a str,
        weight: u8,
        prefix: bool,
    },
    /// 1 not, 2 and, 3 or, 4 phrase (with its distance).
    Operator(u8, i16),
}

/// The text of the item at `items[*at]` as `tsqueryout` writes it (its `infix`), moving
/// `at` past it and its operands.
fn write_query(
    out: &mut String,
    items: &[QueryItem<'_>],
    at: &mut usize,
    parent: u8,
    right_of_phrase: bool,
) -> DecodeResult<()> {
    let item = items.get(*at).ok_or_else(|| invalid("tsquery"))?;
    *at += 1;
    match *item {
        QueryItem::Operand {
            text,
            weight,
            prefix,
        } => {
            push_quoted_lexeme(out, text);
            if weight != 0 || prefix {
                out.push(':');
                if prefix {
                    out.push('*');
                }
                for (bit, letter) in [(8, 'A'), (4, 'B'), (2, 'C'), (1, 'D')] {
                    if weight & bit != 0 {
                        out.push(letter);
                    }
                }
            }
        }
        QueryItem::Operator(1, _) => {
            let open = 4 < parent;
            out.push_str(if open { "( !" } else { "!" });
            write_query(out, items, at, 4, false)?;
            if open {
                out.push_str(" )");
            }
        }
        QueryItem::Operator(op, distance) => {
            let priority = match op {
                2 => 2,
                3 => 1,
                _ => 3,
            };
            let open = priority < parent || (op == 4 && right_of_phrase);
            let mut right = String::new();
            write_query(&mut right, items, at, priority, op == 4)?;
            if open {
                out.push_str("( ");
            }
            write_query(out, items, at, priority, false)?;
            let _ = match op {
                2 => write!(out, " & {right}"),
                3 => write!(out, " | {right}"),
                _ if distance == 1 => write!(out, " <-> {right}"),
                _ => write!(out, " <{distance}> {right}"),
            };
            if open {
                out.push_str(" )");
            }
        }
    }
    Ok(())
}

/// `tsquery`, as `tsqueryout` writes it: `'fat' & ( 'rat' | 'cat' )`.
fn format_tsquery(raw: &[u8]) -> DecodeResult<String> {
    let count = usize::try_from(i32::from_be_bytes(take(raw, "tsquery")?))?;
    let mut buf = &raw[4..];
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        let (&kind, rest) = buf.split_first().ok_or_else(|| invalid("tsquery"))?;
        if kind == 1 {
            let [weight, prefix]: [u8; 2] = take(rest, "tsquery")?;
            let (text, rest) = c_string(&rest[2..], "tsquery")?;
            items.push(QueryItem::Operand {
                text,
                weight,
                prefix: prefix != 0,
            });
            buf = rest;
        } else {
            let (&op, rest) = rest.split_first().ok_or_else(|| invalid("tsquery"))?;
            if op == 4 {
                items.push(QueryItem::Operator(
                    op,
                    i16::from_be_bytes(take(rest, "tsquery")?),
                ));
                buf = &rest[2..];
            } else {
                items.push(QueryItem::Operator(op, 0));
                buf = rest;
            }
        }
    }
    let mut out = String::new();
    if !items.is_empty() {
        write_query(&mut out, &items, &mut 0, 0, false)?;
    }
    Ok(out)
}

/// `pg_snapshot` and `txid_snapshot`: `xmin:xmax:xip,…`.
fn format_snapshot(raw: &[u8]) -> DecodeResult<String> {
    let count = usize::try_from(i32::from_be_bytes(take(raw, "snapshot")?))?;
    let number = |i: usize| -> DecodeResult<u64> {
        Ok(u64::from_be_bytes(take(
            raw.get(4 + i * 8..).unwrap_or_default(),
            "snapshot",
        )?))
    };
    let mut out = format!("{}:{}:", number(0)?, number(1)?);
    for i in 0..count {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "{}", number(2 + i)?);
    }
    Ok(out)
}

/// Renders any PostgreSQL value as text, close to PostgreSQL's own output format.
pub fn format_value(ty: &Type, raw: &[u8]) -> DecodeResult<String> {
    match *ty {
        Type::BOOL => Ok(if pg::bool_from_sql(raw)? {
            "true"
        } else {
            "false"
        }
        .to_owned()),
        Type::INT2 => Ok(pg::int2_from_sql(raw)?.to_string()),
        Type::INT4 => Ok(pg::int4_from_sql(raw)?.to_string()),
        Type::INT8 => Ok(pg::int8_from_sql(raw)?.to_string()),
        Type::OID | Type::REGCLASS | Type::REGTYPE | Type::REGPROC | Type::XID | Type::CID => {
            Ok(pg::oid_from_sql(raw)?.to_string())
        }
        Type::FLOAT4 => Ok(pg::float4_from_sql(raw)?.to_string()),
        Type::FLOAT8 => Ok(pg::float8_from_sql(raw)?.to_string()),
        Type::NUMERIC => Ok(numeric_from_sql(raw)?.to_string()),
        Type::MONEY => {
            let cents = pg::int8_from_sql(raw)?;
            let sign = if cents < 0 { "-" } else { "" };
            Ok(format!(
                "{sign}{}.{:02}",
                (cents / 100).unsigned_abs(),
                (cents % 100).unsigned_abs()
            ))
        }
        Type::CHAR => Ok(char::from(pg::char_from_sql(raw)?.to_be_bytes()[0]).to_string()),
        Type::BYTEA => Ok(format_bytea(raw)),
        Type::JSONB => {
            Ok(std::str::from_utf8(raw.get(1..).ok_or_else(|| invalid("jsonb"))?)?.to_owned())
        }
        Type::DATE => Ok(format_date(pg::date_from_sql(raw)?)),
        Type::TIME => {
            let mut out = String::new();
            write_time(&mut out, pg::time_from_sql(raw)?);
            Ok(out)
        }
        Type::TIMETZ => {
            let b: [u8; 12] = take(raw, "timetz")?;
            let micros = i64::from_be_bytes(b[0..8].try_into()?);
            // The wire format stores seconds *west* of UTC.
            let west = i32::from_be_bytes(b[8..12].try_into()?);
            let mut out = String::new();
            write_time(&mut out, micros);
            write_offset(&mut out, -west);
            Ok(out)
        }
        Type::TIMESTAMP => Ok(format_timestamp(pg::timestamp_from_sql(raw)?, false)),
        Type::TIMESTAMPTZ => Ok(format_timestamp(pg::timestamp_from_sql(raw)?, true)),
        Type::INTERVAL => {
            let (micros, days, months) = interval_from_sql(raw)?;
            Ok(format_interval(micros, days, months))
        }
        Type::UUID => format_uuid(raw),
        Type::INET => format_inet(raw, false),
        Type::CIDR => format_inet(raw, true),
        Type::MACADDR => {
            let b = pg::macaddr_from_sql(raw)?;
            Ok(b.iter()
                .map(|x| format!("{x:02x}"))
                .collect::<Vec<_>>()
                .join(":"))
        }
        Type::MACADDR8 => {
            let b: [u8; 8] = take(raw, "macaddr8")?;
            Ok(b.iter()
                .map(|x| format!("{x:02x}"))
                .collect::<Vec<_>>()
                .join(":"))
        }
        Type::BIT | Type::VARBIT => format_bits(raw),
        Type::PG_LSN => {
            let lsn = u64::from_be_bytes(take(raw, "pg_lsn")?);
            Ok(format!("{:X}/{:X}", lsn >> 32, lsn & 0xffff_ffff))
        }
        Type::TS_VECTOR => format_tsvector(raw),
        Type::RECORD => format_composite(&[], raw),
        Type::TSQUERY => format_tsquery(raw),
        Type::XID8 => Ok(u64::from_be_bytes(take(raw, "xid8")?).to_string()),
        Type::PG_SNAPSHOT | Type::TXID_SNAPSHOT => format_snapshot(raw),
        // A version byte (1), then the text.
        Type::JSONPATH => {
            Ok(std::str::from_utf8(raw.get(1..).ok_or_else(|| invalid("jsonpath"))?)?.to_owned())
        }
        Type::POINT => {
            let p = pg::point_from_sql(raw)?;
            Ok(format!("({},{})", p.x(), p.y()))
        }
        Type::BOX | Type::LSEG | Type::PATH | Type::POLYGON | Type::LINE | Type::CIRCLE => {
            format_geometric(ty, raw)
        }
        _ => match ty.kind() {
            Kind::Array(element) => format_array(element, raw),
            Kind::Range(element) => format_range(element, raw),
            Kind::Domain(base) => format_value(base, raw),
            Kind::Composite(fields) => format_composite(fields, raw),
            // Enums and text-like types (text, varchar, json, xml, citext, ...) are UTF-8 on the wire.
            _ => Ok(std::str::from_utf8(raw).map_or_else(|_| format_bytea(raw), str::to_owned)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numeric_bytes(weight: i16, sign: u16, dscale: u16, digits: &[i16]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&i16::try_from(digits.len()).unwrap().to_be_bytes());
        b.extend_from_slice(&weight.to_be_bytes());
        b.extend_from_slice(&sign.to_be_bytes());
        b.extend_from_slice(&dscale.to_be_bytes());
        for d in digits {
            b.extend_from_slice(&d.to_be_bytes());
        }
        b
    }

    #[test]
    fn formats_bits_addresses_and_lsns() {
        assert_eq!(
            format_value(&Type::BIT, &[0, 0, 0, 4, 0xA0]).unwrap(),
            "1010"
        );
        assert_eq!(
            format_value(&Type::VARBIT, &[0, 0, 0, 10, 0xFF, 0x40]).unwrap(),
            "1111111101"
        );
        assert_eq!(format_value(&Type::VARBIT, &[0, 0, 0, 0]).unwrap(), "");
        assert_eq!(
            format_value(&Type::MACADDR8, &[8, 0, 0x2b, 1, 2, 3, 4, 5]).unwrap(),
            "08:00:2b:01:02:03:04:05"
        );
        assert_eq!(
            format_value(&Type::PG_LSN, &0x16_B374_D848_u64.to_be_bytes()).unwrap(),
            "16/B374D848"
        );
        assert_eq!(format_value(&Type::JSONPATH, b"\x01$.a").unwrap(), "$.a");
    }

    #[test]
    fn formats_text_search_values() {
        let mut vector = vec![0, 0, 0, 2];
        vector.extend_from_slice(b"fox\0\0\x01\0\x03it's\0\0\x02");
        vector.extend_from_slice(&((3u16 << 14) | 2).to_be_bytes());
        vector.extend_from_slice(&5u16.to_be_bytes());
        assert_eq!(
            format_value(&Type::TS_VECTOR, &vector).unwrap(),
            "'fox':3 'it''s':2A,5"
        );

        let operand = |text: &str, weight: u8, prefix: u8| {
            let mut b = vec![1, weight, prefix];
            b.extend_from_slice(text.as_bytes());
            b.push(0);
            b
        };
        let query = |items: &[Vec<u8>]| {
            let mut b = i32::try_from(items.len()).unwrap().to_be_bytes().to_vec();
            items.iter().for_each(|i| b.extend_from_slice(i));
            format_value(&Type::TSQUERY, &b).unwrap()
        };
        let (and, or, not) = (vec![2, 2], vec![2, 3], vec![2, 1]);
        // 'fat' & ( 'rat' | 'cat' ): an operator, its right operand, then its left one.
        assert_eq!(
            query(&[
                and.clone(),
                or.clone(),
                operand("cat", 0, 0),
                operand("rat", 0, 0),
                operand("fat", 0, 0)
            ]),
            "'fat' & ( 'rat' | 'cat' )"
        );
        assert_eq!(
            query(&[not, and, operand("b", 0, 0), operand("a", 12, 1)]),
            "!( 'a':*AB & 'b' )"
        );
        assert_eq!(
            query(&[vec![2, 4, 0, 2], operand("b", 0, 0), operand("a", 0, 0)]),
            "'a' <2> 'b'"
        );
        assert_eq!(
            query(&[or, operand("b", 0, 0), operand("a", 0, 0)]),
            "'a' | 'b'"
        );
        assert_eq!(query(&[]), "");
    }

    #[test]
    fn formats_anonymous_records_by_their_field_types() {
        // (1,"a b") with an int4 field and a text field.
        let mut record = 2_i32.to_be_bytes().to_vec();
        record.extend_from_slice(&23_u32.to_be_bytes());
        record.extend_from_slice(&4_i32.to_be_bytes());
        record.extend_from_slice(&1_i32.to_be_bytes());
        record.extend_from_slice(&25_u32.to_be_bytes());
        record.extend_from_slice(&3_i32.to_be_bytes());
        record.extend_from_slice(b"a b");
        assert_eq!(format_value(&Type::RECORD, &record).unwrap(), "(1,\"a b\")");
    }

    #[test]
    fn numeric_to_string() {
        // 12345678.9012 = 1234 5678 . 9012
        let n = numeric_from_sql(&numeric_bytes(1, 0, 4, &[1234, 5678, 9012])).unwrap();
        assert_eq!(n.to_string(), "12345678.9012");
        // -0.000123 with dscale 6 = 0 . 0001 2300
        let n = numeric_from_sql(&numeric_bytes(-1, 0x4000, 6, &[1, 2300])).unwrap();
        assert_eq!(n.to_string(), "-0.000123");
        // 10000 (weight 1, trailing group omitted on the wire)
        let n = numeric_from_sql(&numeric_bytes(1, 0, 0, &[1])).unwrap();
        assert_eq!(n.to_string(), "10000");
        // zero with scale
        let n = numeric_from_sql(&numeric_bytes(0, 0, 2, &[])).unwrap();
        assert_eq!(n.to_string(), "0.00");
        assert_eq!(
            numeric_from_sql(&numeric_bytes(0, 0xC000, 0, &[])).unwrap(),
            Numeric::NaN
        );
    }

    #[test]
    fn numeric_to_decimal128() {
        let n = numeric_from_sql(&numeric_bytes(1, 0, 4, &[1234, 5678, 9012])).unwrap();
        assert_eq!(n.to_i128(4), Some(123_456_789_012));
        assert_eq!(n.to_i128(6), Some(12_345_678_901_200));
        let n = numeric_from_sql(&numeric_bytes(-1, 0x4000, 6, &[1, 2300])).unwrap();
        assert_eq!(n.to_i128(6), Some(-123));
        assert_eq!(Numeric::NaN.to_i128(2), None);
    }

    #[test]
    fn numeric_typmods() {
        // numeric(12,2)
        assert_eq!(numeric_typmod((12 << 16 | 2) + 4), Some((12, 2)));
        // numeric(5,-2), PostgreSQL 15+
        assert_eq!(
            numeric_typmod((5 << 16 | (-2i32 & 0x7ff)) + 4),
            Some((5, -2))
        );
        assert_eq!(numeric_typmod(-1), None);
    }

    #[test]
    fn dates_and_timestamps() {
        assert_eq!(format_date(0), "2000-01-01");
        assert_eq!(format_date(9527), "2026-01-31");
        assert_eq!(format_date(-730_119), "0001-01-01");
        assert_eq!(format_date(-730_120), "0001-12-31 BC");
        assert_eq!(format_date(i32::MAX), "infinity");
        let ts = (9527 * 86_400 + 13 * 3600 + 37 * 60) * MICROS_PER_SECOND + 120_000;
        assert_eq!(format_timestamp(ts, false), "2026-01-31 13:37:00.12");
        assert_eq!(format_timestamp(ts, true), "2026-01-31 13:37:00.12+00");
        assert_eq!(
            format_timestamp(-MICROS_PER_SECOND, false),
            "1999-12-31 23:59:59"
        );
    }

    #[test]
    fn intervals() {
        assert_eq!(
            format_interval(2 * 3600 * MICROS_PER_SECOND, 1, 0),
            "1 day 02:00:00"
        );
        assert_eq!(format_interval(0, 0, 14), "1 year 2 mons");
        assert_eq!(format_interval(0, 0, 0), "00:00:00");
        assert_eq!(format_interval(-1_500_000, -3, 0), "-3 days -00:00:01.5");
    }

    fn wire(floats: &[f64]) -> Vec<u8> {
        floats.iter().flat_map(|f| f.to_be_bytes()).collect()
    }

    #[test]
    fn geometric_types() {
        let format = |ty: &Type, raw: &[u8]| format_value(ty, raw).unwrap();
        assert_eq!(
            format(&Type::BOX, &wire(&[2.0, 3.0, 0.0, -1.5])),
            "(2,3),(0,-1.5)"
        );
        assert_eq!(
            format(&Type::LSEG, &wire(&[0.0, 0.0, 1.0, 1.0])),
            "[(0,0),(1,1)]"
        );
        let mut polygon = 3_i32.to_be_bytes().to_vec();
        polygon.extend(wire(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0]));
        assert_eq!(format(&Type::POLYGON, &polygon), "((0,0),(1,0),(1,1))");
        let mut open = vec![0];
        open.extend(2_i32.to_be_bytes());
        open.extend(wire(&[0.0, 0.0, 2.5, 1.0]));
        assert_eq!(format(&Type::PATH, &open), "[(0,0),(2.5,1)]");
        assert_eq!(format(&Type::LINE, &wire(&[1.0, -1.0, 0.0])), "{1,-1,0}");
        assert_eq!(format(&Type::CIRCLE, &wire(&[1.0, 2.0, 3.0])), "<(1,2),3>");
        assert!(format_value(&Type::CIRCLE, &wire(&[1.0])).is_err());
    }

    #[test]
    fn element_quoting() {
        let mut out = String::new();
        push_element(&mut out, "a b", Literal::Array(','));
        push_element(&mut out, "plain", Literal::Array(','));
        push_element(&mut out, "q\"x", Literal::Array(','));
        assert_eq!(out, "\"a b\"plain\"q\\\"x\"");
        // As psql prints them: parentheses are plain in arrays, braces in composites.
        let quoted = |s: &str, literal| needs_quotes(s, literal);
        assert!(!quoted("a(b)", Literal::Array(',')));
        assert!(quoted("a{b}", Literal::Array(',')));
        assert!(quoted("NULL", Literal::Array(',')));
        assert!(!quoted("(0,0)", Literal::Array(';')));
        assert!(quoted("a;b", Literal::Array(';')));
        assert!(quoted("a(b)", Literal::Composite));
        assert!(!quoted("{a}", Literal::Composite));
        assert!(!quoted("null", Literal::Composite));
        assert!(quoted("", Literal::Composite));
        // Only ASCII white space is quoted, as PostgreSQL does.
        assert!(!quoted("a\u{a0}b", Literal::Array(',')));
        assert!(!quoted("a\u{3000}b", Literal::Composite));
        assert!(quoted("a\tb", Literal::Composite));
        let mut out = String::new();
        push_element(&mut out, "say \"hi\" \\", Literal::Composite);
        assert_eq!(out, r#""say ""hi"" \\""#);
    }

    /// An array on the wire: `bounds` are `(lower bound, length)` per dimension.
    fn array_wire(element: &Type, bounds: &[(i32, i32)], values: &[Option<&[u8]>]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(i32::try_from(bounds.len()).unwrap().to_be_bytes());
        b.extend(i32::from(values.iter().any(Option::is_none)).to_be_bytes());
        b.extend(element.oid().to_be_bytes());
        for (lower, len) in bounds {
            b.extend(len.to_be_bytes());
            b.extend(lower.to_be_bytes());
        }
        for v in values {
            match v {
                Some(v) => {
                    b.extend(i32::try_from(v.len()).unwrap().to_be_bytes());
                    b.extend_from_slice(v);
                }
                None => b.extend((-1_i32).to_be_bytes()),
            }
        }
        b
    }

    #[test]
    fn arrays_as_postgresql_prints_them() {
        let format = |array: &Type, bounds: &[(i32, i32)], values: &[Option<&[u8]>]| {
            let Kind::Array(element) = array.kind() else {
                panic!("{array} is not an array type");
            };
            format_value(array, &array_wire(element, bounds, values)).unwrap()
        };
        let int4s = &Type::INT4_ARRAY;
        let one = 1_i32.to_be_bytes();
        let two = 2_i32.to_be_bytes();
        assert_eq!(format(int4s, &[], &[]), "{}");
        assert_eq!(format(int4s, &[(1, 2)], &[Some(&one), None]), "{1,NULL}");
        assert_eq!(
            format(
                int4s,
                &[(1, 2), (1, 2)],
                &[Some(&one), Some(&two), None, Some(&one)]
            ),
            "{{1,2},{NULL,1}}"
        );
        // Bounds other than 1 are kept, or saving the text back would move the elements.
        assert_eq!(
            format(int4s, &[(0, 2)], &[Some(&one), Some(&two)]),
            "[0:1]={1,2}"
        );
        assert_eq!(
            format(int4s, &[(1, 1), (-2, 2)], &[Some(&one), Some(&two)]),
            "[1:1][-2:-1]={{1,2}}"
        );
        assert_eq!(
            format(
                &Type::TEXT_ARRAY,
                &[(1, 4)],
                &[Some(b"a(b)"), Some(b"b c"), Some(b"null"), Some(b"")]
            ),
            r#"{a(b),"b c","null",""}"#
        );
        // Boxes hold commas: their elements are separated by `;`, as PostgreSQL reads them.
        let a_box = wire(&[1.0, 1.0, 0.0, 0.0]);
        let b_box = wire(&[2.0, 2.0, 1.0, 1.0]);
        assert_eq!(
            format(&Type::BOX_ARRAY, &[(1, 2)], &[Some(&a_box), Some(&b_box)]),
            "{(1,1),(0,0);(2,2),(1,1)}"
        );
    }
}
