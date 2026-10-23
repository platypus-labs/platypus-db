//! MySQL result columns into Arrow.
//!
//! Queries use the text protocol (every statement can run that way, unlike prepared
//! statements), so each value arrives as text and is parsed by the builder of its column.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::ArrayRef;
use arrow_array::builder::{
    BinaryBuilder, Date32Builder, Decimal128Builder, Float32Builder, Float64Builder, Int8Builder,
    Int16Builder, Int32Builder, Int64Builder, StringBuilder, TimestampMicrosecondBuilder,
    UInt8Builder, UInt16Builder, UInt32Builder, UInt64Builder,
};
use arrow_schema::{DataType, Field, TimeUnit};
use mysql_async::consts::{ColumnFlags, ColumnType};

/// Collation id of the `binary` character set: bytes, not text.
const BINARY_CHARSET: u16 = 63;

/// Sessions use UTC, so `TIMESTAMP` values are instants in this zone.
pub const SESSION_TIME_ZONE: &str = "+00:00";

/// The column facts the mapping needs, decoupled from `mysql_async::Column` for tests.
#[derive(Debug, Clone)]
pub struct ColumnSpec {
    pub name: String,
    pub column_type: ColumnType,
    pub flags: ColumnFlags,
    pub length: u32,
    pub decimals: u8,
    pub charset: u16,
}

impl From<&mysql_async::Column> for ColumnSpec {
    fn from(c: &mysql_async::Column) -> Self {
        Self {
            name: c.name_str().into_owned(),
            column_type: c.column_type(),
            flags: c.flags(),
            length: c.column_length(),
            decimals: c.decimals(),
            charset: c.character_set(),
        }
    }
}

impl ColumnSpec {
    fn unsigned(&self) -> bool {
        self.flags.contains(ColumnFlags::UNSIGNED_FLAG)
    }

    fn binary(&self) -> bool {
        self.charset == BINARY_CHARSET
    }

    /// `(precision, scale)` of a `DECIMAL` that fits `Decimal128`.
    fn decimal(&self) -> Option<(u8, i8)> {
        let scale = self.decimals;
        let overhead = u32::from(scale > 0) + u32::from(!self.unsigned());
        let precision = u8::try_from(self.length.checked_sub(overhead)?).ok()?;
        (scale <= 30 && precision >= scale && (1..=38).contains(&precision))
            .then(|| (precision, i8::try_from(scale).unwrap_or(0)))
    }

    /// The type name shown in the grid header.
    #[must_use]
    pub fn type_name(&self) -> String {
        use ColumnType as T;
        let base = match self.column_type {
            T::MYSQL_TYPE_TINY => "TINYINT",
            T::MYSQL_TYPE_SHORT => "SMALLINT",
            T::MYSQL_TYPE_INT24 => "MEDIUMINT",
            T::MYSQL_TYPE_LONG => "INT",
            T::MYSQL_TYPE_LONGLONG => "BIGINT",
            T::MYSQL_TYPE_FLOAT => "FLOAT",
            T::MYSQL_TYPE_DOUBLE => "DOUBLE",
            T::MYSQL_TYPE_DECIMAL | T::MYSQL_TYPE_NEWDECIMAL => {
                return match self.decimal() {
                    Some((p, s)) => format!("DECIMAL({p},{s})"),
                    None => "DECIMAL".to_owned(),
                };
            }
            T::MYSQL_TYPE_DATE | T::MYSQL_TYPE_NEWDATE => "DATE",
            T::MYSQL_TYPE_TIME | T::MYSQL_TYPE_TIME2 => "TIME",
            T::MYSQL_TYPE_DATETIME | T::MYSQL_TYPE_DATETIME2 => "DATETIME",
            T::MYSQL_TYPE_TIMESTAMP | T::MYSQL_TYPE_TIMESTAMP2 => "TIMESTAMP",
            T::MYSQL_TYPE_YEAR => "YEAR",
            T::MYSQL_TYPE_BIT => "BIT",
            T::MYSQL_TYPE_JSON => "JSON",
            T::MYSQL_TYPE_GEOMETRY => "GEOMETRY",
            T::MYSQL_TYPE_VECTOR => "VECTOR",
            T::MYSQL_TYPE_NULL => "NULL",
            _ if self.flags.contains(ColumnFlags::ENUM_FLAG) => "ENUM",
            _ if self.flags.contains(ColumnFlags::SET_FLAG) => "SET",
            T::MYSQL_TYPE_TINY_BLOB
            | T::MYSQL_TYPE_MEDIUM_BLOB
            | T::MYSQL_TYPE_LONG_BLOB
            | T::MYSQL_TYPE_BLOB => {
                if self.binary() {
                    "BLOB"
                } else {
                    "TEXT"
                }
            }
            T::MYSQL_TYPE_STRING => {
                if self.binary() {
                    "BINARY"
                } else {
                    "CHAR"
                }
            }
            _ => {
                if self.binary() {
                    "VARBINARY"
                } else {
                    "VARCHAR"
                }
            }
        };
        let is_int = matches!(
            self.column_type,
            T::MYSQL_TYPE_TINY
                | T::MYSQL_TYPE_SHORT
                | T::MYSQL_TYPE_INT24
                | T::MYSQL_TYPE_LONG
                | T::MYSQL_TYPE_LONGLONG
        );
        if is_int && self.unsigned() {
            format!("{base} UNSIGNED")
        } else {
            base.to_owned()
        }
    }
}

#[derive(Debug)]
pub enum ColumnBuilder {
    Int8(Int8Builder),
    Int16(Int16Builder),
    Int32(Int32Builder),
    Int64(Int64Builder),
    UInt8(UInt8Builder),
    UInt16(UInt16Builder),
    UInt32(UInt32Builder),
    UInt64(UInt64Builder),
    Float32(Float32Builder),
    Float64(Float64Builder),
    Decimal(Decimal128Builder, (u8, i8)),
    Date(Date32Builder),
    Timestamp(TimestampMicrosecondBuilder, Option<Arc<str>>),
    Bit(UInt64Builder),
    Text(StringBuilder),
    Binary(BinaryBuilder),
}

/// The Arrow field and an empty builder for a result column.
#[must_use]
pub fn column_builder(spec: &ColumnSpec) -> (Field, ColumnBuilder) {
    use ColumnType as T;
    let unsigned = spec.unsigned();
    let builder = match spec.column_type {
        T::MYSQL_TYPE_TINY if unsigned => ColumnBuilder::UInt8(UInt8Builder::new()),
        T::MYSQL_TYPE_TINY => ColumnBuilder::Int8(Int8Builder::new()),
        T::MYSQL_TYPE_SHORT if unsigned => ColumnBuilder::UInt16(UInt16Builder::new()),
        T::MYSQL_TYPE_SHORT => ColumnBuilder::Int16(Int16Builder::new()),
        T::MYSQL_TYPE_INT24 | T::MYSQL_TYPE_LONG if unsigned => {
            ColumnBuilder::UInt32(UInt32Builder::new())
        }
        T::MYSQL_TYPE_INT24 | T::MYSQL_TYPE_LONG => ColumnBuilder::Int32(Int32Builder::new()),
        T::MYSQL_TYPE_LONGLONG if unsigned => ColumnBuilder::UInt64(UInt64Builder::new()),
        T::MYSQL_TYPE_LONGLONG => ColumnBuilder::Int64(Int64Builder::new()),
        T::MYSQL_TYPE_YEAR => ColumnBuilder::UInt16(UInt16Builder::new()),
        T::MYSQL_TYPE_FLOAT => ColumnBuilder::Float32(Float32Builder::new()),
        T::MYSQL_TYPE_DOUBLE => ColumnBuilder::Float64(Float64Builder::new()),
        T::MYSQL_TYPE_DECIMAL | T::MYSQL_TYPE_NEWDECIMAL => match spec.decimal() {
            Some((p, s)) => ColumnBuilder::Decimal(
                Decimal128Builder::new()
                    .with_precision_and_scale(p, s)
                    .unwrap_or_default(),
                (p, s),
            ),
            None => ColumnBuilder::Text(StringBuilder::new()),
        },
        T::MYSQL_TYPE_DATE | T::MYSQL_TYPE_NEWDATE => ColumnBuilder::Date(Date32Builder::new()),
        T::MYSQL_TYPE_DATETIME | T::MYSQL_TYPE_DATETIME2 => {
            ColumnBuilder::Timestamp(TimestampMicrosecondBuilder::new(), None)
        }
        T::MYSQL_TYPE_TIMESTAMP | T::MYSQL_TYPE_TIMESTAMP2 => {
            let tz: Arc<str> = Arc::from(SESSION_TIME_ZONE);
            ColumnBuilder::Timestamp(
                TimestampMicrosecondBuilder::new().with_timezone(Arc::clone(&tz)),
                Some(tz),
            )
        }
        T::MYSQL_TYPE_BIT => ColumnBuilder::Bit(UInt64Builder::new()),
        T::MYSQL_TYPE_GEOMETRY | T::MYSQL_TYPE_VECTOR => {
            ColumnBuilder::Binary(BinaryBuilder::new())
        }
        // JSON reports the binary charset but always holds UTF-8 text, so it is not here.
        T::MYSQL_TYPE_TINY_BLOB
        | T::MYSQL_TYPE_MEDIUM_BLOB
        | T::MYSQL_TYPE_LONG_BLOB
        | T::MYSQL_TYPE_BLOB
        | T::MYSQL_TYPE_STRING
        | T::MYSQL_TYPE_VAR_STRING
        | T::MYSQL_TYPE_VARCHAR
            if spec.binary() =>
        {
            ColumnBuilder::Binary(BinaryBuilder::new())
        }
        _ => ColumnBuilder::Text(StringBuilder::new()),
    };
    let field = Field::new(&spec.name, builder.data_type(), true).with_metadata(HashMap::from([(
        platypus_core::META_DB_TYPE.to_owned(),
        spec.type_name(),
    )]));
    (field, builder)
}

/// Days from 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn num(s: &str) -> Option<i64> {
    s.parse().ok()
}

/// `YYYY-MM-DD` to days since the epoch; `None` for zero or invalid dates.
fn parse_date(s: &str) -> Option<i64> {
    let mut parts = s.splitn(3, '-');
    let (y, m, d) = (
        num(parts.next()?)?,
        num(parts.next()?)?,
        num(parts.next()?)?,
    );
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then(|| days_from_civil(y, m, d))
}

/// `YYYY-MM-DD HH:MM:SS[.ffffff]` to microseconds since the epoch.
fn parse_datetime(s: &str) -> Option<i64> {
    let (date, time) = s.split_once(' ').unwrap_or((s, "00:00:00"));
    let days = parse_date(date)?;
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut parts = hms.splitn(3, ':');
    let (h, m, sec) = (
        num(parts.next()?)?,
        num(parts.next()?)?,
        num(parts.next()?)?,
    );
    let micros = if frac.is_empty() {
        0
    } else {
        let digits: String = frac.chars().chain(std::iter::repeat('0')).take(6).collect();
        num(&digits)?
    };
    Some(((days * 24 + h) * 60 + m) * 60_000_000 + sec * 1_000_000 + micros)
}

/// A decimal literal to its unscaled `i128` at `scale`; extra digits are rejected.
fn parse_decimal(s: &str, scale: i8) -> Option<i128> {
    let scale = usize::try_from(scale).ok()?;
    let (negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let (int, frac) = digits.split_once('.').unwrap_or((digits, ""));
    if frac.len() > scale || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value: i128 = 0;
    for b in int.bytes().chain(frac.bytes()) {
        value = value.checked_mul(10)?.checked_add(i128::from(b - b'0'))?;
    }
    for _ in frac.len()..scale {
        value = value.checked_mul(10)?;
    }
    Some(if negative { -value } else { value })
}

macro_rules! parse_into {
    ($builder:expr, $text:expr) => {{
        let text = $text?;
        $builder.append_value(text.parse().map_err(|e| format!("`{text}`: {e}"))?);
    }};
}

impl ColumnBuilder {
    fn data_type(&self) -> DataType {
        match self {
            Self::Int8(_) => DataType::Int8,
            Self::Int16(_) => DataType::Int16,
            Self::Int32(_) => DataType::Int32,
            Self::Int64(_) => DataType::Int64,
            Self::UInt8(_) => DataType::UInt8,
            Self::UInt16(_) => DataType::UInt16,
            Self::UInt32(_) => DataType::UInt32,
            Self::UInt64(_) | Self::Bit(_) => DataType::UInt64,
            Self::Float32(_) => DataType::Float32,
            Self::Float64(_) => DataType::Float64,
            Self::Decimal(_, (p, s)) => DataType::Decimal128(*p, *s),
            Self::Date(_) => DataType::Date32,
            Self::Timestamp(_, tz) => DataType::Timestamp(TimeUnit::Microsecond, tz.clone()),
            Self::Text(_) => DataType::Utf8,
            Self::Binary(_) => DataType::Binary,
        }
    }

    /// Appends a value in text-protocol form; `None` is SQL `NULL`.
    pub fn append(&mut self, value: Option<&[u8]>) -> Result<(), String> {
        let Some(bytes) = value else {
            self.append_null();
            return Ok(());
        };
        let text = || std::str::from_utf8(bytes).map_err(|e| e.to_string());
        match self {
            Self::Int8(b) => parse_into!(b, text()),
            Self::Int16(b) => parse_into!(b, text()),
            Self::Int32(b) => parse_into!(b, text()),
            Self::Int64(b) => parse_into!(b, text()),
            Self::UInt8(b) => parse_into!(b, text()),
            Self::UInt16(b) => parse_into!(b, text()),
            Self::UInt32(b) => parse_into!(b, text()),
            Self::UInt64(b) => parse_into!(b, text()),
            Self::Float32(b) => parse_into!(b, text()),
            Self::Float64(b) => parse_into!(b, text()),
            Self::Decimal(b, (_, scale)) => {
                let text = text()?;
                b.append_value(
                    parse_decimal(text, *scale).ok_or_else(|| format!("bad decimal `{text}`"))?,
                );
            }
            // Zero dates such as `0000-00-00` have no calendar equivalent.
            Self::Date(b) => {
                b.append_option(parse_date(text()?).and_then(|d| i32::try_from(d).ok()));
            }
            Self::Timestamp(b, _) => b.append_option(parse_datetime(text()?)),
            Self::Bit(b) => {
                if bytes.len() > 8 {
                    return Err(format!("BIT value of {} bytes", bytes.len()));
                }
                b.append_value(bytes.iter().fold(0, |acc, &x| (acc << 8) | u64::from(x)));
            }
            Self::Text(b) => b.append_value(String::from_utf8_lossy(bytes)),
            Self::Binary(b) => b.append_value(bytes),
        }
        Ok(())
    }

    fn append_null(&mut self) {
        match self {
            Self::Int8(b) => b.append_null(),
            Self::Int16(b) => b.append_null(),
            Self::Int32(b) => b.append_null(),
            Self::Int64(b) => b.append_null(),
            Self::UInt8(b) => b.append_null(),
            Self::UInt16(b) => b.append_null(),
            Self::UInt32(b) => b.append_null(),
            Self::UInt64(b) | Self::Bit(b) => b.append_null(),
            Self::Float32(b) => b.append_null(),
            Self::Float64(b) => b.append_null(),
            Self::Decimal(b, _) => b.append_null(),
            Self::Date(b) => b.append_null(),
            Self::Timestamp(b, _) => b.append_null(),
            Self::Text(b) => b.append_null(),
            Self::Binary(b) => b.append_null(),
        }
    }

    pub fn finish(&mut self) -> ArrayRef {
        match self {
            Self::Int8(b) => Arc::new(b.finish()),
            Self::Int16(b) => Arc::new(b.finish()),
            Self::Int32(b) => Arc::new(b.finish()),
            Self::Int64(b) => Arc::new(b.finish()),
            Self::UInt8(b) => Arc::new(b.finish()),
            Self::UInt16(b) => Arc::new(b.finish()),
            Self::UInt32(b) => Arc::new(b.finish()),
            Self::UInt64(b) | Self::Bit(b) => Arc::new(b.finish()),
            Self::Float32(b) => Arc::new(b.finish()),
            Self::Float64(b) => Arc::new(b.finish()),
            Self::Decimal(b, _) => Arc::new(b.finish()),
            Self::Date(b) => Arc::new(b.finish()),
            Self::Timestamp(b, _) => Arc::new(b.finish()),
            Self::Text(b) => Arc::new(b.finish()),
            Self::Binary(b) => Arc::new(b.finish()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(column_type: ColumnType, flags: ColumnFlags, length: u32, decimals: u8) -> ColumnSpec {
        ColumnSpec {
            name: "c".to_owned(),
            column_type,
            flags,
            length,
            decimals,
            charset: 45,
        }
    }

    #[test]
    fn parses_dates_and_times() {
        assert_eq!(parse_date("1970-01-01"), Some(0));
        assert_eq!(parse_date("2000-03-01"), Some(11_017));
        assert_eq!(parse_date("0000-00-00"), None);
        assert_eq!(
            parse_datetime("1970-01-02 00:00:01.5"),
            Some(86_401_500_000)
        );
        assert_eq!(parse_datetime("1969-12-31 23:59:59"), Some(-1_000_000));
    }

    #[test]
    fn parses_decimals() {
        assert_eq!(parse_decimal("123.45", 2), Some(12_345));
        assert_eq!(parse_decimal("-0.5", 3), Some(-500));
        assert_eq!(parse_decimal("7", 2), Some(700));
        assert_eq!(parse_decimal("1.234", 2), None);
        assert_eq!(parse_decimal("1e5", 0), None);
    }

    #[test]
    fn maps_column_types() {
        let decimal = spec(
            ColumnType::MYSQL_TYPE_NEWDECIMAL,
            ColumnFlags::empty(),
            22,
            6,
        );
        assert_eq!(decimal.decimal(), Some((20, 6)));
        assert_eq!(decimal.type_name(), "DECIMAL(20,6)");
        let (field, _) = column_builder(&decimal);
        assert_eq!(field.data_type(), &DataType::Decimal128(20, 6));

        let ubig = spec(
            ColumnType::MYSQL_TYPE_LONGLONG,
            ColumnFlags::UNSIGNED_FLAG,
            20,
            0,
        );
        assert_eq!(ubig.type_name(), "BIGINT UNSIGNED");
        assert_eq!(column_builder(&ubig).0.data_type(), &DataType::UInt64);

        let mut blob = spec(ColumnType::MYSQL_TYPE_BLOB, ColumnFlags::empty(), 65_535, 0);
        assert_eq!(column_builder(&blob).0.data_type(), &DataType::Utf8);
        blob.charset = BINARY_CHARSET;
        assert_eq!(column_builder(&blob).0.data_type(), &DataType::Binary);
        assert_eq!(blob.type_name(), "BLOB");
    }

    #[test]
    fn decodes_values() {
        let (_, mut bit) = column_builder(&spec(
            ColumnType::MYSQL_TYPE_BIT,
            ColumnFlags::empty(),
            8,
            0,
        ));
        bit.append(Some(&[0x01, 0x02])).unwrap();
        bit.append(None).unwrap();
        let array = bit.finish();
        let bits = array
            .as_any()
            .downcast_ref::<arrow_array::UInt64Array>()
            .unwrap();
        assert_eq!(bits.iter().collect::<Vec<_>>(), [Some(258), None]);

        let (_, mut int) = column_builder(&spec(
            ColumnType::MYSQL_TYPE_LONG,
            ColumnFlags::empty(),
            11,
            0,
        ));
        assert!(int.append(Some(b"12x")).is_err());
    }
}
