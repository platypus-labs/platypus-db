//! SQL Server result columns into Arrow.
//!
//! TDS sends typed values: they are appended to the builder of their column without going
//! through text. Dates count days from 0001-01-01 (`date`, `datetime2`) or 1900-01-01
//! (`datetime`, `smalldatetime`); `datetimeoffset` values arrive in UTC.
//!
//! Values that an Arrow type would change are text as SQL Server writes them, which it
//! reads back exactly: `datetime2(7)` (100 ns, finer than Arrow's microseconds over its
//! range) and `datetimeoffset` (each value with its own offset). `datetime` counts 1/300
//! of a second, shown and written back in milliseconds as SQL Server rounds them.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use arrow_array::ArrayRef;
use arrow_array::builder::{
    BinaryBuilder, BooleanBuilder, Date32Builder, Decimal128Builder, Float32Builder,
    Float64Builder, Int16Builder, Int32Builder, Int64Builder, StringBuilder,
    Time64NanosecondBuilder, TimestampMicrosecondBuilder, UInt8Builder,
};
use arrow_schema::{DataType, Field, TimeUnit};
use tiberius::ColumnData;
use tiberius::ColumnType as T;

/// `datetimeoffset` values are instants, kept in this zone.
pub const UTC: &str = "+00:00";

/// Days from 0001-01-01 to 1970-01-01.
const DAYS_FROM_YEAR_1: i64 = 719_162;
/// Days from 1900-01-01 to 1970-01-01.
const DAYS_FROM_1900: i64 = 25_567;
const MICROS_PER_DAY: i64 = 86_400_000_000;

/// The column facts the mapping needs, decoupled from `tiberius::Column` for tests.
#[derive(Debug, Clone)]
pub struct ColumnSpec {
    pub name: String,
    pub column_type: T,
    pub precision: Option<u8>,
    pub scale: Option<u8>,
    /// Bytes; `None` for `max` types.
    pub max_length: Option<usize>,
}

impl From<&tiberius::Column> for ColumnSpec {
    fn from(c: &tiberius::Column) -> Self {
        Self {
            name: c.name().to_owned(),
            column_type: c.column_type(),
            precision: c.precision(),
            scale: c.scale(),
            max_length: c.max_length(),
        }
    }
}

impl ColumnSpec {
    /// `(n)` of a sized type, `(max)` otherwise; `chars` halves the bytes of `nchar` types.
    fn length(&self, chars: bool) -> String {
        match self.max_length {
            Some(bytes) if chars => format!("({})", bytes / 2),
            Some(bytes) => format!("({bytes})"),
            None => "(max)".to_owned(),
        }
    }

    /// The type name shown in the grid header, as SQL Server spells it.
    #[must_use]
    pub fn type_name(&self) -> String {
        let scaled = |name: &str| match self.scale {
            Some(s) => format!("{name}({s})"),
            None => name.to_owned(),
        };
        match self.column_type {
            T::Bit | T::Bitn => "bit".to_owned(),
            T::Int1 => "tinyint".to_owned(),
            T::Int2 => "smallint".to_owned(),
            // `NULL` constants come as `int`.
            T::Null | T::Int4 | T::Intn => "int".to_owned(),
            T::Int8 => "bigint".to_owned(),
            T::Float4 => "real".to_owned(),
            T::Float8 | T::Floatn => "float".to_owned(),
            T::Money | T::Money4 => "money".to_owned(),
            T::Decimaln | T::Numericn => match (self.precision, self.scale) {
                (Some(p), Some(s)) => format!("decimal({p},{s})"),
                _ => "decimal".to_owned(),
            },
            T::Datetime4 => "smalldatetime".to_owned(),
            T::Datetime | T::Datetimen => "datetime".to_owned(),
            T::Daten => "date".to_owned(),
            T::Timen => scaled("time"),
            T::Datetime2 => scaled("datetime2"),
            T::DatetimeOffsetn => scaled("datetimeoffset"),
            T::Guid => "uniqueidentifier".to_owned(),
            T::BigVarChar => format!("varchar{}", self.length(false)),
            T::BigChar => format!("char{}", self.length(false)),
            T::NVarchar => format!("nvarchar{}", self.length(true)),
            T::NChar => format!("nchar{}", self.length(true)),
            T::BigVarBin => format!("varbinary{}", self.length(false)),
            T::BigBinary => format!("binary{}", self.length(false)),
            T::Xml => "xml".to_owned(),
            T::Text => "text".to_owned(),
            T::NText => "ntext".to_owned(),
            T::Image => "image".to_owned(),
            // geometry, geography, hierarchyid and CLR types, as their serialized bytes.
            T::Udt => "udt".to_owned(),
            T::SSVariant => "sql_variant".to_owned(),
        }
    }
}

#[derive(Debug)]
pub enum ColumnBuilder {
    Bool(BooleanBuilder),
    UInt8(UInt8Builder),
    Int16(Int16Builder),
    Int32(Int32Builder),
    Int64(Int64Builder),
    Float32(Float32Builder),
    Float64(Float64Builder),
    /// The scale of the column, to align values that carry another.
    Decimal(Decimal128Builder, (u8, i8)),
    /// `money`, a count of ten-thousandths: a decimal with 4 decimals.
    Money(Decimal128Builder),
    Date(Date32Builder),
    Time(Time64NanosecondBuilder),
    /// With a time zone for `datetimeoffset`.
    Timestamp(TimestampMicrosecondBuilder, bool),
    Text(StringBuilder),
    Binary(BinaryBuilder),
}

/// The Arrow field and an empty builder for a result column.
#[must_use]
pub fn column_builder(spec: &ColumnSpec) -> (Field, ColumnBuilder) {
    let builder = match spec.column_type {
        T::Bit | T::Bitn => ColumnBuilder::Bool(BooleanBuilder::new()),
        T::Int1 => ColumnBuilder::UInt8(UInt8Builder::new()),
        T::Int2 => ColumnBuilder::Int16(Int16Builder::new()),
        T::Int4 | T::Null => ColumnBuilder::Int32(Int32Builder::new()),
        T::Int8 | T::Intn => ColumnBuilder::Int64(Int64Builder::new()),
        T::Float4 => ColumnBuilder::Float32(Float32Builder::new()),
        T::Float8 | T::Floatn => ColumnBuilder::Float64(Float64Builder::new()),
        T::Money | T::Money4 => ColumnBuilder::Money(decimal_builder(19, 4)),
        T::Decimaln | T::Numericn => {
            let precision = spec.precision.unwrap_or(38).clamp(1, 38);
            let scale = i8::try_from(spec.scale.unwrap_or(0)).unwrap_or(0);
            ColumnBuilder::Decimal(decimal_builder(precision, scale), (precision, scale))
        }
        T::Daten => ColumnBuilder::Date(Date32Builder::new()),
        T::Timen => ColumnBuilder::Time(Time64NanosecondBuilder::new()),
        T::Datetime2 if spec.scale.is_none_or(|s| s > 6) => {
            ColumnBuilder::Text(StringBuilder::new())
        }
        T::Datetime4 | T::Datetime | T::Datetimen | T::Datetime2 => {
            ColumnBuilder::Timestamp(TimestampMicrosecondBuilder::new(), false)
        }
        T::BigVarBin | T::BigBinary | T::Image | T::Udt => {
            ColumnBuilder::Binary(BinaryBuilder::new())
        }
        // Each `datetimeoffset` value keeps its own offset, as text.
        T::DatetimeOffsetn
        | T::Guid
        | T::BigVarChar
        | T::BigChar
        | T::NVarchar
        | T::NChar
        | T::Xml
        | T::Text
        | T::NText
        | T::SSVariant => ColumnBuilder::Text(StringBuilder::new()),
    };
    let field = Field::new(&spec.name, builder.data_type(), true).with_metadata(HashMap::from([(
        platypus_core::META_DB_TYPE.to_owned(),
        spec.type_name(),
    )]));
    (field, builder)
}

fn decimal_builder(precision: u8, scale: i8) -> Decimal128Builder {
    Decimal128Builder::new()
        .with_precision_and_scale(precision, scale)
        .unwrap_or_default()
}

/// Whether the value is SQL `NULL`, whatever its variant.
fn is_null(value: &ColumnData<'_>) -> bool {
    match value {
        ColumnData::U8(v) => v.is_none(),
        ColumnData::I16(v) => v.is_none(),
        ColumnData::I32(v) => v.is_none(),
        ColumnData::I64(v) => v.is_none(),
        ColumnData::F32(v) => v.is_none(),
        ColumnData::F64(v) => v.is_none(),
        ColumnData::Bit(v) => v.is_none(),
        ColumnData::String(v) => v.is_none(),
        ColumnData::Guid(v) => v.is_none(),
        ColumnData::Binary(v) => v.is_none(),
        ColumnData::Numeric(v) => v.is_none(),
        ColumnData::Xml(v) => v.is_none(),
        ColumnData::DateTime(v) => v.is_none(),
        ColumnData::SmallDateTime(v) => v.is_none(),
        ColumnData::Time(v) => v.is_none(),
        ColumnData::Date(v) => v.is_none(),
        ColumnData::DateTime2(v) => v.is_none(),
        ColumnData::DateTimeOffset(v) => v.is_none(),
    }
}

/// Any integer value as `i64`.
fn integer(value: &ColumnData<'_>) -> Option<i64> {
    match value {
        ColumnData::U8(Some(v)) => Some(i64::from(*v)),
        ColumnData::I16(Some(v)) => Some(i64::from(*v)),
        ColumnData::I32(Some(v)) => Some(i64::from(*v)),
        ColumnData::I64(Some(v)) => Some(*v),
        ColumnData::Bit(Some(v)) => Some(i64::from(*v)),
        _ => None,
    }
}

fn float(value: &ColumnData<'_>) -> Option<f64> {
    match value {
        ColumnData::F32(Some(v)) => Some(f64::from(*v)),
        ColumnData::F64(Some(v)) => Some(*v),
        _ => None,
    }
}

/// Days since 1970-01-01 of a `date`.
fn date_days(date: tiberius::time::Date) -> i64 {
    i64::from(date.days()) - DAYS_FROM_YEAR_1
}

/// Nanoseconds since midnight of a `time` with `scale` fractional digits.
fn time_nanos(time: tiberius::time::Time) -> i64 {
    let increments = i64::try_from(time.increments()).unwrap_or(i64::MAX);
    let scale = u32::from(time.scale().min(9));
    increments.saturating_mul(10_i64.pow(9 - scale))
}

/// Microseconds since the epoch of a date and time value.
fn timestamp_micros(value: &ColumnData<'_>) -> Option<i64> {
    Some(match value {
        ColumnData::DateTime(Some(dt)) => {
            let days = i64::from(dt.days()) - DAYS_FROM_1900;
            // 1/300 of a second, rounded to the millisecond as SQL Server shows it: a
            // string with more digits is no `datetime` to it.
            let millis = (i64::from(dt.seconds_fragments()) * 10 + 1) / 3;
            days * MICROS_PER_DAY + millis * 1000
        }
        ColumnData::SmallDateTime(Some(dt)) => {
            let days = i64::from(dt.days()) - DAYS_FROM_1900;
            // Minutes since midnight.
            days * MICROS_PER_DAY + i64::from(dt.seconds_fragments()) * 60_000_000
        }
        ColumnData::DateTime2(Some(dt)) => {
            date_days(dt.date()) * MICROS_PER_DAY + time_nanos(dt.time()) / 1000
        }
        ColumnData::DateTimeOffset(Some(dto)) => {
            let dt = dto.datetime2();
            date_days(dt.date()) * MICROS_PER_DAY + time_nanos(dt.time()) / 1000
        }
        ColumnData::Date(Some(d)) => date_days(*d) * MICROS_PER_DAY,
        _ => return None,
    })
}

/// A time of day `nanos` after midnight with `scale` fractional digits, e.g. `10:00:00.5`.
fn time_text(nanos: i64, scale: u8) -> String {
    let secs = nanos / 1_000_000_000;
    let mut s = format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    if scale > 0 {
        let frac = format!("{:09}", nanos % 1_000_000_000);
        let _ = write!(s, ".{}", &frac[..usize::from(scale.min(9))]);
    }
    s
}

/// The date `days` after 1970-01-01 and the time `nanos` after its midnight (carried into
/// the next or previous day when beyond it), as SQL Server writes them.
fn datetime_text(days: i64, nanos: i64, scale: u8) -> Option<String> {
    const NANOS_PER_DAY: i64 = MICROS_PER_DAY * 1000;
    let days = days + nanos.div_euclid(NANOS_PER_DAY);
    let days = i32::try_from(days).ok()?;
    let date = arrow_array::temporal_conversions::date32_to_datetime(days)?.date();
    Some(format!(
        "{date} {}",
        time_text(nanos.rem_euclid(NANOS_PER_DAY), scale)
    ))
}

/// `value` with `scale` decimals as text, e.g. `-1.50`.
fn decimal_text(value: i128, scale: u8) -> String {
    let digits = value.unsigned_abs().to_string();
    let scale = usize::from(scale);
    let sign = if value < 0 { "-" } else { "" };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (int, frac) = padded.split_at(padded.len() - scale);
    format!("{sign}{int}.{frac}")
}

/// The value as text, for `sql_variant` columns whose rows have different types.
fn text(value: &ColumnData<'_>) -> Option<String> {
    use arrow_array::temporal_conversions::{date32_to_datetime, timestamp_us_to_datetime};
    Some(match value {
        ColumnData::String(Some(s)) => s.to_string(),
        ColumnData::Guid(Some(g)) => g.to_string().to_uppercase(),
        ColumnData::Xml(Some(x)) => x.to_string(),
        ColumnData::Numeric(Some(n)) => decimal_text(n.value(), n.scale()),
        ColumnData::F32(Some(v)) => v.to_string(),
        ColumnData::F64(Some(v)) => v.to_string(),
        ColumnData::Binary(Some(b)) => b.iter().fold(String::from("0x"), |mut s, byte| {
            let _ = write!(s, "{byte:02X}");
            s
        }),
        ColumnData::Time(Some(t)) => time_text(time_nanos(*t), t.scale()),
        ColumnData::Date(Some(d)) => date32_to_datetime(i32::try_from(date_days(*d)).ok()?)?
            .date()
            .to_string(),
        ColumnData::DateTime2(Some(dt)) => datetime_text(
            date_days(dt.date()),
            time_nanos(dt.time()),
            dt.time().scale(),
        )?,
        ColumnData::DateTimeOffset(Some(dto)) => {
            let offset = i64::from(dto.offset());
            let utc = dto.datetime2();
            let nanos = time_nanos(utc.time()) + offset * 60_000_000_000;
            let local = datetime_text(date_days(utc.date()), nanos, utc.time().scale())?;
            let sign = if offset < 0 { '-' } else { '+' };
            format!(
                "{local} {sign}{:02}:{:02}",
                offset.abs() / 60,
                offset.abs() % 60
            )
        }
        ColumnData::DateTime(Some(_)) | ColumnData::SmallDateTime(Some(_)) => {
            timestamp_us_to_datetime(timestamp_micros(value)?)?.to_string()
        }
        other => integer(other)?.to_string(),
    })
}

/// `numeric` at another scale than its column's, e.g. from `sql_variant`.
fn rescale(value: i128, from: u8, to: i8) -> Option<i128> {
    let to = u8::try_from(to).ok()?;
    if from <= to {
        value.checked_mul(10_i128.checked_pow(u32::from(to - from))?)
    } else {
        Some(value / 10_i128.checked_pow(u32::from(from - to))?)
    }
}

fn mismatch(value: &ColumnData<'_>, data_type: &DataType) -> String {
    format!(
        "unexpected {} value for {data_type} column",
        variant_name(value)
    )
}

fn variant_name(value: &ColumnData<'_>) -> &'static str {
    match value {
        ColumnData::U8(_) => "tinyint",
        ColumnData::I16(_) => "smallint",
        ColumnData::I32(_) => "int",
        ColumnData::I64(_) => "bigint",
        ColumnData::F32(_) => "real",
        ColumnData::F64(_) => "float",
        ColumnData::Bit(_) => "bit",
        ColumnData::String(_) => "string",
        ColumnData::Guid(_) => "uniqueidentifier",
        ColumnData::Binary(_) => "binary",
        ColumnData::Numeric(_) => "numeric",
        ColumnData::Xml(_) => "xml",
        ColumnData::DateTime(_) => "datetime",
        ColumnData::SmallDateTime(_) => "smalldatetime",
        ColumnData::Time(_) => "time",
        ColumnData::Date(_) => "date",
        ColumnData::DateTime2(_) => "datetime2",
        ColumnData::DateTimeOffset(_) => "datetimeoffset",
    }
}

impl ColumnBuilder {
    fn data_type(&self) -> DataType {
        match self {
            Self::Bool(_) => DataType::Boolean,
            Self::UInt8(_) => DataType::UInt8,
            Self::Int16(_) => DataType::Int16,
            Self::Int32(_) => DataType::Int32,
            Self::Int64(_) => DataType::Int64,
            Self::Float32(_) => DataType::Float32,
            Self::Float64(_) => DataType::Float64,
            Self::Decimal(_, (p, s)) => DataType::Decimal128(*p, *s),
            Self::Money(_) => DataType::Decimal128(19, 4),
            Self::Date(_) => DataType::Date32,
            Self::Time(_) => DataType::Time64(TimeUnit::Nanosecond),
            Self::Timestamp(_, zoned) => {
                DataType::Timestamp(TimeUnit::Microsecond, zoned.then(|| Arc::from(UTC)))
            }
            Self::Text(_) => DataType::Utf8,
            Self::Binary(_) => DataType::Binary,
        }
    }

    fn append_null(&mut self) {
        match self {
            Self::Bool(b) => b.append_null(),
            Self::UInt8(b) => b.append_null(),
            Self::Int16(b) => b.append_null(),
            Self::Int32(b) => b.append_null(),
            Self::Int64(b) => b.append_null(),
            Self::Float32(b) => b.append_null(),
            Self::Float64(b) => b.append_null(),
            Self::Decimal(b, _) | Self::Money(b) => b.append_null(),
            Self::Date(b) => b.append_null(),
            Self::Time(b) => b.append_null(),
            Self::Timestamp(b, _) => b.append_null(),
            Self::Text(b) => b.append_null(),
            Self::Binary(b) => b.append_null(),
        }
    }

    /// Appends one value of the column.
    pub fn append(&mut self, value: &ColumnData<'_>) -> Result<(), String> {
        if is_null(value) {
            self.append_null();
            return Ok(());
        }
        let data_type = self.data_type();
        let wrong = || mismatch(value, &data_type);
        let out_of_range = |n: i64| format!("{n} is out of range for {data_type}");
        match self {
            Self::Bool(b) => b.append_value(integer(value).ok_or_else(wrong)? != 0),
            Self::UInt8(b) => {
                let n = integer(value).ok_or_else(wrong)?;
                b.append_value(u8::try_from(n).map_err(|_| out_of_range(n))?);
            }
            Self::Int16(b) => {
                let n = integer(value).ok_or_else(wrong)?;
                b.append_value(i16::try_from(n).map_err(|_| out_of_range(n))?);
            }
            Self::Int32(b) => {
                let n = integer(value).ok_or_else(wrong)?;
                b.append_value(i32::try_from(n).map_err(|_| out_of_range(n))?);
            }
            Self::Int64(b) => b.append_value(integer(value).ok_or_else(wrong)?),
            #[expect(clippy::cast_possible_truncation, reason = "real columns hold f32")]
            Self::Float32(b) => b.append_value(float(value).ok_or_else(wrong)? as f32),
            Self::Float64(b) => b.append_value(float(value).ok_or_else(wrong)?),
            Self::Decimal(b, (_, scale)) => {
                let ColumnData::Numeric(Some(n)) = value else {
                    return Err(wrong());
                };
                let v = rescale(n.value(), n.scale(), *scale)
                    .ok_or_else(|| format!("{n:?} does not fit the column's scale"))?;
                b.append_value(v);
            }
            Self::Money(b) => {
                let ColumnData::Numeric(Some(n)) = value else {
                    return Err(wrong());
                };
                let v = rescale(n.value(), n.scale(), 4)
                    .ok_or_else(|| format!("{n:?} does not fit money"))?;
                b.append_value(v);
            }
            Self::Date(b) => {
                let ColumnData::Date(Some(d)) = value else {
                    return Err(wrong());
                };
                b.append_value(i32::try_from(date_days(*d)).map_err(|e| e.to_string())?);
            }
            Self::Time(b) => {
                let ColumnData::Time(Some(t)) = value else {
                    return Err(wrong());
                };
                b.append_value(time_nanos(*t));
            }
            Self::Timestamp(b, _) => b.append_value(timestamp_micros(value).ok_or_else(wrong)?),
            Self::Text(b) => b.append_value(text(value).ok_or_else(wrong)?),
            Self::Binary(b) => match value {
                ColumnData::Binary(Some(bytes)) => b.append_value(bytes),
                ColumnData::String(Some(s)) => b.append_value(s.as_bytes()),
                _ => return Err(wrong()),
            },
        }
        Ok(())
    }

    pub fn finish(&mut self) -> ArrayRef {
        match self {
            Self::Bool(b) => Arc::new(b.finish()),
            Self::UInt8(b) => Arc::new(b.finish()),
            Self::Int16(b) => Arc::new(b.finish()),
            Self::Int32(b) => Arc::new(b.finish()),
            Self::Int64(b) => Arc::new(b.finish()),
            Self::Float32(b) => Arc::new(b.finish()),
            Self::Float64(b) => Arc::new(b.finish()),
            Self::Decimal(b, (p, s)) => Arc::new(
                b.finish()
                    .with_precision_and_scale(*p, *s)
                    .unwrap_or_else(|_| unreachable_decimal()),
            ),
            Self::Money(b) => Arc::new(
                b.finish()
                    .with_precision_and_scale(19, 4)
                    .unwrap_or_else(|_| unreachable_decimal()),
            ),
            Self::Date(b) => Arc::new(b.finish()),
            Self::Time(b) => Arc::new(b.finish()),
            Self::Timestamp(b, zoned) => {
                let array = b.finish();
                Arc::new(if *zoned {
                    array.with_timezone(UTC)
                } else {
                    array
                })
            }
            Self::Text(b) => Arc::new(b.finish()),
            Self::Binary(b) => Arc::new(b.finish()),
        }
    }
}

/// Precision and scale were validated when the builder was made.
fn unreachable_decimal() -> arrow_array::Decimal128Array {
    arrow_array::Decimal128Array::from(Vec::<i128>::new())
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use arrow_array::Array;
    use arrow_cast::display::array_value_to_string;
    use tiberius::numeric::Numeric;
    use tiberius::time::{Date, DateTime, DateTime2, DateTimeOffset, SmallDateTime, Time};

    use super::*;

    fn spec(column_type: T) -> ColumnSpec {
        ColumnSpec {
            name: "c".to_owned(),
            column_type,
            precision: None,
            scale: None,
            max_length: None,
        }
    }

    /// The column's values as the grid shows them.
    fn shown(spec: &ColumnSpec, values: &[ColumnData<'_>]) -> Vec<String> {
        let (field, mut builder) = column_builder(spec);
        for v in values {
            builder.append(v).unwrap();
        }
        let array = builder.finish();
        assert_eq!(array.data_type(), field.data_type());
        (0..array.len())
            .map(|i| {
                if array.is_null(i) {
                    "NULL".to_owned()
                } else {
                    array_value_to_string(&array, i).unwrap()
                }
            })
            .collect()
    }

    #[test]
    fn names_types() {
        let mut s = spec(T::Decimaln);
        s.precision = Some(12);
        s.scale = Some(2);
        assert_eq!(s.type_name(), "decimal(12,2)");
        let mut s = spec(T::NVarchar);
        s.max_length = Some(100);
        assert_eq!(s.type_name(), "nvarchar(50)");
        assert_eq!(spec(T::BigVarBin).type_name(), "varbinary(max)");
        let mut s = spec(T::Datetime2);
        s.scale = Some(7);
        assert_eq!(s.type_name(), "datetime2(7)");
    }

    #[test]
    fn reads_numbers() {
        assert_eq!(
            shown(
                &spec(T::Intn),
                &[
                    ColumnData::I32(Some(-4)),
                    ColumnData::I64(Some(1 << 40)),
                    ColumnData::I64(None)
                ]
            ),
            ["-4", "1099511627776", "NULL"]
        );
        assert_eq!(shown(&spec(T::Int1), &[ColumnData::U8(Some(255))]), ["255"]);
        assert_eq!(
            shown(
                &spec(T::Bitn),
                &[ColumnData::Bit(Some(true)), ColumnData::Bit(None)]
            ),
            ["true", "NULL"]
        );
        assert_eq!(
            shown(
                &spec(T::Floatn),
                &[ColumnData::F32(Some(1.5)), ColumnData::F64(Some(2.25))]
            ),
            ["1.5", "2.25"]
        );
        let mut s = spec(T::Numericn);
        s.precision = Some(10);
        s.scale = Some(2);
        assert_eq!(
            shown(
                &s,
                &[
                    ColumnData::Numeric(Some(Numeric::new_with_scale(1250, 2))),
                    ColumnData::Numeric(Some(Numeric::new_with_scale(-5, 1))),
                ]
            ),
            ["12.50", "-0.50"]
        );
        // The largest money, beyond what a float holds exactly.
        assert_eq!(
            shown(
                &spec(T::Money),
                &[ColumnData::Numeric(Some(Numeric::new_with_scale(
                    i128::from(i64::MAX),
                    4
                )))]
            ),
            ["922337203685477.5807"]
        );
        let (_, mut int) = column_builder(&spec(T::Int2));
        assert!(int.append(&ColumnData::I32(Some(70_000))).is_err());
        assert!(int.append(&ColumnData::String(Some("x".into()))).is_err());
    }

    #[test]
    fn reads_dates_and_times() {
        // 2024-03-01 is day 738_945 from 0001-01-01 and 45_350 from 1900-01-01.
        let date = Date::new(738_945);
        let ten = Time::new(36_000_000_000 + 1_234_567, 7);
        assert_eq!(
            shown(&spec(T::Daten), &[ColumnData::Date(Some(date))]),
            ["2024-03-01"]
        );
        assert_eq!(
            shown(&spec(T::Timen), &[ColumnData::Time(Some(ten))]),
            ["01:00:00.123456700"]
        );
        // datetime2(7) keeps its 100 ns as text; with up to 6 digits it is a timestamp.
        let mut seven = spec(T::Datetime2);
        seven.scale = Some(7);
        assert_eq!(
            shown(
                &seven,
                &[ColumnData::DateTime2(Some(DateTime2::new(date, ten)))]
            ),
            ["2024-03-01 01:00:00.1234567"]
        );
        let mut six = spec(T::Datetime2);
        six.scale = Some(6);
        let micro = Time::new(3_600_000_000 + 123_456, 6);
        assert_eq!(
            shown(
                &six,
                &[ColumnData::DateTime2(Some(DateTime2::new(date, micro)))]
            ),
            ["2024-03-01T01:00:00.123456"]
        );
        // datetime in milliseconds, as SQL Server shows and reads it: 1/300 s is .003,
        // 299/300 s is .997.
        assert_eq!(
            shown(
                &spec(T::Datetimen),
                &[
                    ColumnData::DateTime(Some(DateTime::new(45_350, 300 * 3600 + 1))),
                    ColumnData::DateTime(Some(DateTime::new(45_350, 300 * 3600 + 299))),
                    ColumnData::DateTime(Some(DateTime::new(45_350, 300 * 86_400 - 1))),
                    ColumnData::SmallDateTime(Some(SmallDateTime::new(45_350, 90))),
                ]
            ),
            [
                "2024-03-01T01:00:00.003",
                "2024-03-01T01:00:00.997",
                "2024-03-01T23:59:59.997",
                "2024-03-01T01:30:00"
            ]
        );
        // 10:00 at +02:00 is 08:00 UTC, which is what the protocol sends; the value keeps
        // its offset. Late at night UTC is the next day at +02:00.
        let utc = DateTime2::new(date, Time::new(8 * 3600, 0));
        let dto = ColumnData::DateTimeOffset(Some(DateTimeOffset::new(utc, 120)));
        let late = DateTime2::new(date, Time::new(23 * 3600 * 10_000_000 + 5, 7));
        assert_eq!(
            shown(
                &spec(T::DatetimeOffsetn),
                &[
                    dto.clone(),
                    ColumnData::DateTimeOffset(Some(DateTimeOffset::new(late, 120))),
                    ColumnData::DateTimeOffset(Some(DateTimeOffset::new(utc, -330))),
                ]
            ),
            [
                "2024-03-01 10:00:00 +02:00",
                "2024-03-02 01:00:00.0000005 +02:00",
                "2024-03-01 02:30:00 -05:30"
            ]
        );
        assert_eq!(
            shown(&spec(T::SSVariant), &[dto]),
            ["2024-03-01 10:00:00 +02:00"]
        );
    }

    #[test]
    fn reads_text_binary_and_variants() {
        let guid = tiberius::Uuid::parse_str("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11").unwrap();
        assert_eq!(
            shown(
                &spec(T::Guid),
                &[ColumnData::Guid(Some(guid)), ColumnData::String(None)]
            ),
            ["A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11", "NULL"]
        );
        assert_eq!(
            shown(
                &spec(T::BigVarBin),
                &[ColumnData::Binary(Some(Cow::Borrowed(&[0, 255, 16])))]
            ),
            ["00ff10"]
        );
        assert_eq!(
            shown(
                &spec(T::SSVariant),
                &[
                    ColumnData::I32(Some(7)),
                    ColumnData::String(Some("x".into())),
                    ColumnData::Numeric(Some(Numeric::new_with_scale(-123, 2))),
                    ColumnData::Time(Some(Time::new(36_000 * 10 + 5, 1))),
                ]
            ),
            ["7", "x", "-1.23", "10:00:00.5"]
        );
    }

    #[test]
    fn formats_decimals() {
        assert_eq!(decimal_text(5, 2), "0.05");
        assert_eq!(decimal_text(-1234, 0), "-1234");
        assert_eq!(rescale(15, 1, 3), Some(1500));
        assert_eq!(rescale(1500, 3, 1), Some(15));
    }
}
