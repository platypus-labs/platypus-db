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

/// Text that does not need quoting inside an array or composite literal.
fn needs_quotes(s: &str, delimiter: char) -> bool {
    s.is_empty()
        || s.eq_ignore_ascii_case("null")
        || s.chars().any(|c| {
            c == delimiter || matches!(c, '"' | '\\' | '{' | '}' | '(' | ')') || c.is_whitespace()
        })
}

fn push_element(out: &mut String, value: &str, delimiter: char) {
    if needs_quotes(value, delimiter) {
        out.push('"');
        for c in value.chars() {
            if matches!(c, '"' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
    } else {
        out.push_str(value);
    }
}

fn format_array(element: &Type, raw: &[u8]) -> DecodeResult<String> {
    let array = pg::array_from_sql(raw)?;
    let dims: Vec<usize> = array
        .dimensions()
        .map(|d| Ok(usize::try_from(d.len).unwrap_or(0)))
        .collect()?;
    let mut values = array.values();
    if dims.is_empty() {
        return Ok("{}".to_owned());
    }
    let mut out = String::new();
    write_array_level(&mut out, element, &dims, &mut values)?;
    Ok(out)
}

fn write_array_level(
    out: &mut String,
    element: &Type,
    dims: &[usize],
    values: &mut pg::ArrayValues<'_>,
) -> DecodeResult<()> {
    out.push('{');
    for i in 0..dims[0] {
        if i > 0 {
            out.push(',');
        }
        if dims.len() > 1 {
            write_array_level(out, element, &dims[1..], values)?;
        } else {
            match values.next()?.ok_or_else(|| invalid("array"))? {
                None => out.push_str("NULL"),
                Some(raw) => push_element(out, &format_value(element, raw)?, ','),
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
        let ty = fields
            .get(i)
            .map_or(&Type::TEXT, tokio_postgres::types::Field::type_);
        push_element(&mut out, &format_value(ty, value)?, ',');
    }
    out.push(')');
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
        Type::POINT => {
            let p = pg::point_from_sql(raw)?;
            Ok(format!("({},{})", p.x(), p.y()))
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

    #[test]
    fn element_quoting() {
        let mut out = String::new();
        push_element(&mut out, "a b", ',');
        push_element(&mut out, "plain", ',');
        push_element(&mut out, "q\"x", ',');
        assert_eq!(out, "\"a b\"plain\"q\\\"x\"");
    }
}
