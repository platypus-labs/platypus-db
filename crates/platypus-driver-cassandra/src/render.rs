//! CQL values as text and JSON: the form the grid shows and the editor reads back.
//!
//! Numbers wider than any Arrow type (`varint`, `decimal`), identifiers, addresses and
//! durations go to the grid as text; collections, tuples, vectors and user-defined types as
//! JSON text, so that one cell holds the whole value and edits it as such.

use std::fmt::Write as _;

use scylla::value::{CqlDate, CqlDecimal, CqlDuration, CqlTime, CqlTimestamp, CqlValue};
use serde_json::{Map, Number, Value};

const MILLIS_PER_DAY: i64 = 86_400_000;
const NANOS_PER_SECOND: i64 = 1_000_000_000;

/// Days since 1970-01-01 of a CQL `date`, which counts from 2^31 days before.
#[must_use]
pub const fn days_since_epoch(date: CqlDate) -> i64 {
    date.0 as i64 - (1 << 31)
}

/// The proleptic Gregorian date `days` after 1970-01-01.
#[must_use]
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
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

/// `2024-03-09`.
#[must_use]
pub fn date_text(date: CqlDate) -> String {
    let (y, m, d) = civil_from_days(days_since_epoch(date));
    format!("{y:04}-{m:02}-{d:02}")
}

/// `13:05:09.250`: milliseconds, and more digits only when the value has them.
#[must_use]
pub fn time_text(time: CqlTime) -> String {
    let nanos = time.0.rem_euclid(86_400 * NANOS_PER_SECOND);
    let secs = nanos / NANOS_PER_SECOND;
    let frac = nanos % NANOS_PER_SECOND;
    let mut out = format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    if frac != 0 {
        let digits = format!("{frac:09}");
        let digits = if frac % 1_000_000 == 0 {
            &digits[..3]
        } else if frac % 1000 == 0 {
            &digits[..6]
        } else {
            &digits
        };
        out.push('.');
        out.push_str(digits);
    }
    out
}

/// `2024-03-09T13:05:09.250Z`, always in UTC.
#[must_use]
pub fn timestamp_text(ts: CqlTimestamp) -> String {
    let days = ts.0.div_euclid(MILLIS_PER_DAY);
    let millis = ts.0.rem_euclid(MILLIS_PER_DAY);
    let (y, m, d) = civil_from_days(days);
    let secs = millis / 1000;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        millis % 1000
    )
}

/// The decimal digits of a two's complement big-endian integer.
#[must_use]
pub fn varint_text(bytes: &[u8]) -> String {
    let negative = bytes.first().is_some_and(|b| b & 0x80 != 0);
    // Magnitude as base 10^9 limbs, least significant first.
    let mut magnitude: Vec<u8> = bytes.to_vec();
    if negative {
        for b in &mut magnitude {
            *b = !*b;
        }
        for b in magnitude.iter_mut().rev() {
            let (next, carry) = b.overflowing_add(1);
            *b = next;
            if !carry {
                break;
            }
        }
    }
    let mut limbs: Vec<u32> = vec![0];
    for byte in magnitude {
        let mut carry = u64::from(byte);
        for limb in &mut limbs {
            let v = u64::from(*limb) * 256 + carry;
            *limb = (v % 1_000_000_000) as u32;
            carry = v / 1_000_000_000;
        }
        while carry > 0 {
            limbs.push((carry % 1_000_000_000) as u32);
            carry /= 1_000_000_000;
        }
    }
    let mut out = String::new();
    if negative && limbs.iter().any(|&l| l != 0) {
        out.push('-');
    }
    let mut limbs = limbs.iter().rev().skip_while(|&&l| l == 0);
    match limbs.next() {
        None => out.push('0'),
        Some(first) => {
            let _ = write!(out, "{first}");
            for limb in limbs {
                let _ = write!(out, "{limb:09}");
            }
        }
    }
    out
}

/// A `decimal` as its plain digits (`12.50`), never in exponent form.
#[must_use]
pub fn decimal_text(decimal: &CqlDecimal) -> String {
    let (bytes, scale) = decimal.as_signed_be_bytes_slice_and_exponent();
    let digits = varint_text(bytes);
    let (sign, digits) = digits
        .strip_prefix('-')
        .map_or(("", digits.as_str()), |d| ("-", d));
    if scale <= 0 {
        let zeros = "0".repeat(usize::try_from(-i64::from(scale)).unwrap_or(0));
        let zeros = if digits == "0" { "" } else { zeros.as_str() };
        return format!("{sign}{digits}{zeros}");
    }
    let scale = usize::try_from(scale).unwrap_or(0);
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (whole, frac) = padded.split_at(padded.len() - scale);
    format!("{sign}{whole}.{frac}")
}

/// A `duration` in the unit form CQL reads back: `1y2mo3d4h5m6s7ms8us9ns`.
#[must_use]
pub fn duration_text(d: &CqlDuration) -> String {
    let negative = d.months < 0 || d.days < 0 || d.nanoseconds < 0;
    let months = d.months.unsigned_abs();
    let days = d.days.unsigned_abs();
    let nanos = d.nanoseconds.unsigned_abs();
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    let units = [
        (u64::from(months / 12), "y"),
        (u64::from(months % 12), "mo"),
        (u64::from(days), "d"),
        (nanos / 3_600_000_000_000, "h"),
        (nanos / 60_000_000_000 % 60, "m"),
        (nanos / 1_000_000_000 % 60, "s"),
        (nanos / 1_000_000 % 1000, "ms"),
        (nanos / 1000 % 1000, "us"),
        (nanos % 1000, "ns"),
    ];
    for (n, unit) in units {
        if n != 0 {
            let _ = write!(out, "{n}{unit}");
        }
    }
    if out.is_empty() || out == "-" {
        "0s".clone_into(&mut out);
    }
    out
}

/// `0x` and the bytes in hex.
#[must_use]
pub fn blob_text(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(2 + bytes.len() * 2);
    out.push_str("0x");
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// The words CQL writes for the numbers that are none: `NaN` and `Infinity`.
fn special_float(v: f64) -> Option<&'static str> {
    if v.is_nan() {
        Some("NaN")
    } else if v.is_infinite() {
        Some(if v > 0.0 { "Infinity" } else { "-Infinity" })
    } else {
        None
    }
}

/// A `float` as CQL writes it: the shortest text that reads back as the same number.
fn float_text(v: f32) -> String {
    special_float(f64::from(v)).map_or_else(|| v.to_string(), str::to_owned)
}

/// A `double` as CQL writes it.
fn double_text(v: f64) -> String {
    special_float(v).map_or_else(|| v.to_string(), str::to_owned)
}

/// The text of a scalar value, `None` for the values that are structures.
#[must_use]
pub fn scalar_text(value: &CqlValue) -> Option<String> {
    Some(match value {
        CqlValue::Ascii(s) | CqlValue::Text(s) => s.clone(),
        CqlValue::Boolean(b) => b.to_string(),
        CqlValue::TinyInt(v) => v.to_string(),
        CqlValue::SmallInt(v) => v.to_string(),
        CqlValue::Int(v) => v.to_string(),
        CqlValue::BigInt(v) => v.to_string(),
        CqlValue::Counter(v) => v.0.to_string(),
        CqlValue::Float(v) => float_text(*v),
        CqlValue::Double(v) => double_text(*v),
        CqlValue::Varint(v) => varint_text(v.as_signed_bytes_be_slice()),
        CqlValue::Decimal(v) => decimal_text(v),
        CqlValue::Uuid(v) => v.to_string(),
        CqlValue::Timeuuid(v) => v.to_string(),
        CqlValue::Inet(v) => v.to_string(),
        CqlValue::Date(v) => date_text(*v),
        CqlValue::Time(v) => time_text(*v),
        CqlValue::Timestamp(v) => timestamp_text(*v),
        CqlValue::Duration(v) => duration_text(v),
        CqlValue::Blob(v) => blob_text(v),
        CqlValue::Empty => String::new(),
        _ => return None,
    })
}

/// A value as JSON, the form collections, tuples, vectors and user-defined types take in a
/// cell. Numbers that JSON cannot hold exactly stay text.
#[must_use]
pub fn json(value: &CqlValue) -> Value {
    match value {
        CqlValue::Boolean(b) => Value::Bool(*b),
        CqlValue::TinyInt(v) => Value::from(*v),
        CqlValue::SmallInt(v) => Value::from(*v),
        CqlValue::Int(v) => Value::from(*v),
        CqlValue::BigInt(v) => Value::from(*v),
        CqlValue::Counter(v) => Value::from(v.0),
        CqlValue::Float(v) => Number::from_f64(f64::from(*v))
            .and_then(|_| format!("{v}").parse::<Number>().ok())
            .map_or_else(|| Value::String(float_text(*v)), Value::Number),
        CqlValue::Double(v) => {
            Number::from_f64(*v).map_or_else(|| Value::String(double_text(*v)), Value::Number)
        }
        CqlValue::List(items) | CqlValue::Set(items) | CqlValue::Vector(items) => {
            Value::Array(items.iter().map(json).collect())
        }
        CqlValue::Tuple(items) => Value::Array(
            items
                .iter()
                .map(|v| v.as_ref().map_or(Value::Null, json))
                .collect(),
        ),
        CqlValue::Map(entries) => Value::Object(map_object(entries)),
        CqlValue::UserDefinedType { fields, .. } => Value::Object(
            fields
                .iter()
                .map(|(name, v)| (name.clone(), v.as_ref().map_or(Value::Null, json)))
                .collect(),
        ),
        other => scalar_text(other).map_or(Value::Null, Value::String),
    }
}

/// A map as a JSON object: its keys as their text. Keys that are structures themselves are
/// written as JSON text.
fn map_object(entries: &[(CqlValue, CqlValue)]) -> Map<String, Value> {
    entries
        .iter()
        .map(|(k, v)| {
            let key = scalar_text(k).unwrap_or_else(|| json(k).to_string());
            (key, json(v))
        })
        .collect()
}

/// The text a cell shows for `value`: the plain text of a scalar, JSON for the rest.
#[must_use]
pub fn text(value: &CqlValue) -> String {
    scalar_text(value).unwrap_or_else(|| json(value).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_times() {
        assert_eq!(date_text(CqlDate(1 << 31)), "1970-01-01");
        assert_eq!(date_text(CqlDate((1 << 31) + 19_791)), "2024-03-09");
        assert_eq!(date_text(CqlDate((1 << 31) - 1)), "1969-12-31");
        assert_eq!(
            timestamp_text(CqlTimestamp(1_709_989_509_250)),
            "2024-03-09T13:05:09.250Z"
        );
        assert_eq!(timestamp_text(CqlTimestamp(-1)), "1969-12-31T23:59:59.999Z");
        assert_eq!(time_text(CqlTime(47_109_250_000_000)), "13:05:09.250");
        assert_eq!(time_text(CqlTime(3_600_000_000_000)), "01:00:00");
        assert_eq!(time_text(CqlTime(1)), "00:00:00.000000001");
    }

    #[test]
    fn big_numbers() {
        assert_eq!(varint_text(&[0]), "0");
        assert_eq!(varint_text(&[0x7f]), "127");
        assert_eq!(varint_text(&[0xff]), "-1");
        assert_eq!(varint_text(&[0x80]), "-128");
        assert_eq!(varint_text(&[0x01, 0x00]), "256");
        // 2^70
        assert_eq!(
            varint_text(&[0x40, 0, 0, 0, 0, 0, 0, 0, 0]),
            "1180591620717411303424"
        );
        let dec = |bytes: &[u8], scale| {
            decimal_text(&CqlDecimal::from_signed_be_bytes_slice_and_exponent(
                bytes, scale,
            ))
        };
        assert_eq!(dec(&[0x04, 0xd2], 2), "12.34");
        assert_eq!(dec(&[0x05], 3), "0.005");
        assert_eq!(dec(&[0xfb], 1), "-0.5");
        assert_eq!(dec(&[0x05], -2), "500");
        assert_eq!(dec(&[0], 2), "0.00");
    }

    #[test]
    fn durations_and_blobs() {
        let d = CqlDuration {
            months: 14,
            days: 3,
            nanoseconds: 4 * 3_600_000_000_000 + 5_000_000,
        };
        assert_eq!(duration_text(&d), "1y2mo3d4h5ms");
        assert_eq!(
            duration_text(&CqlDuration {
                months: 0,
                days: 0,
                nanoseconds: 0
            }),
            "0s"
        );
        assert_eq!(blob_text(&[0xde, 0xad]), "0xdead");
    }

    #[test]
    fn collections_are_json() {
        let value = CqlValue::Map(vec![
            (
                CqlValue::Text("a".into()),
                CqlValue::List(vec![CqlValue::Int(1)]),
            ),
            (CqlValue::Text("b".into()), CqlValue::List(vec![])),
        ]);
        assert_eq!(text(&value), r#"{"a":[1],"b":[]}"#);
        let udt = CqlValue::UserDefinedType {
            keyspace: "k".into(),
            name: "t".into(),
            fields: vec![("n".into(), Some(CqlValue::Int(1))), ("m".into(), None)],
        };
        // Whatever order the fields come out in, they are the same JSON.
        let parsed: Value = serde_json::from_str(&text(&udt)).unwrap();
        assert_eq!(parsed, serde_json::json!({"m": null, "n": 1}));
        assert_eq!(text(&CqlValue::Double(f64::NAN)), "NaN");
        assert_eq!(text(&CqlValue::Float(1.5)), "1.5");
    }
}
