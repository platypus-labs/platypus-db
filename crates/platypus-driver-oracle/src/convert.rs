//! Oracle values into Arrow arrays. Numbers with a declared precision keep it (integers,
//! decimals); a `NUMBER` without one can hold any of them, so it is read as exact text.
//! Times without a zone are naive timestamps; `TIMESTAMP WITH [LOCAL] TIME ZONE` is
//! converted to UTC.

use std::sync::Arc;

use arrow_array::builder::{
    ArrayBuilder, BinaryBuilder, BooleanBuilder, Decimal128Builder, Float32Builder, Float64Builder,
    Int64Builder, StringBuilder, TimestampMicrosecondBuilder, TimestampSecondBuilder,
};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use oracle::sql_type::{OracleType, Timestamp};
use oracle::{ColumnInfo, SqlValue};
use platypus_core::{Error, META_DB_TYPE, Result};

/// How a column's values are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Int64,
    Decimal(u8, i8),
    Float32,
    Float64,
    Text,
    Binary,
    /// `DATE`, which holds seconds.
    Date,
    Timestamp,
    /// With a time zone, converted to UTC.
    Zoned,
    Boolean,
}

/// The kind of an Oracle type.
#[must_use]
pub fn kind(t: &OracleType) -> Kind {
    match *t {
        OracleType::Number(p, 0) if (1..=18).contains(&p) => Kind::Int64,
        OracleType::Number(p, s) if (1..=38).contains(&p) && s > 0 && s.unsigned_abs() <= p => {
            Kind::Decimal(p, s)
        }
        OracleType::Int64 => Kind::Int64,
        OracleType::BinaryFloat => Kind::Float32,
        OracleType::BinaryDouble | OracleType::Float(_) => Kind::Float64,
        OracleType::Raw(_) | OracleType::LongRaw | OracleType::BLOB => Kind::Binary,
        OracleType::Date => Kind::Date,
        OracleType::Timestamp(_) => Kind::Timestamp,
        OracleType::TimestampTZ(_) | OracleType::TimestampLTZ(_) => Kind::Zoned,
        OracleType::Boolean => Kind::Boolean,
        _ => Kind::Text,
    }
}

fn data_type(kind: Kind) -> DataType {
    match kind {
        Kind::Int64 => DataType::Int64,
        Kind::Decimal(p, s) => DataType::Decimal128(p, s),
        Kind::Float32 => DataType::Float32,
        Kind::Float64 => DataType::Float64,
        Kind::Text => DataType::Utf8,
        Kind::Binary => DataType::Binary,
        Kind::Date => DataType::Timestamp(TimeUnit::Second, None),
        Kind::Timestamp => DataType::Timestamp(TimeUnit::Microsecond, None),
        Kind::Zoned => DataType::Timestamp(TimeUnit::Microsecond, Some("+00:00".into())),
        Kind::Boolean => DataType::Boolean,
    }
}

/// The Arrow schema of a result, each field keeping the Oracle type it came from.
#[must_use]
pub fn schema(columns: &[ColumnInfo]) -> SchemaRef {
    let fields: Vec<Field> = columns
        .iter()
        .map(|c| {
            Field::new(c.name(), data_type(kind(c.oracle_type())), c.nullable())
                .with_metadata([(META_DB_TYPE.to_owned(), c.oracle_type().to_string())])
        })
        .collect();
    Arc::new(Schema::new(fields))
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Seconds since the epoch of `t`'s date and time, ignoring its time zone.
fn epoch_seconds(t: &Timestamp) -> i64 {
    days_from_civil(i64::from(t.year()), t.month(), t.day()) * 86_400
        + i64::from(t.hour()) * 3600
        + i64::from(t.minute()) * 60
        + i64::from(t.second())
}

/// Microseconds since the epoch; in UTC when `zoned`.
#[must_use]
pub fn epoch_micros(t: &Timestamp, zoned: bool) -> i64 {
    let mut seconds = epoch_seconds(t);
    if zoned {
        seconds -= i64::from(t.tz_offset());
    }
    seconds * 1_000_000 + i64::from(t.nanosecond() / 1000)
}

/// The decimal `text` (`-12.5`, `.5`, `1E+3`) as an integer of `scale` decimal places;
/// `None` when it does not fit.
#[must_use]
pub fn decimal(text: &str, scale: i8) -> Option<i128> {
    let text = text.trim();
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(i) => (&text[..i], text[i + 1..].parse::<i32>().ok()?),
        None => (text, 0),
    };
    let (negative, digits) = match mantissa.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, mantissa.strip_prefix('+').unwrap_or(mantissa)),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let shift = i32::from(scale) + exponent - i32::try_from(fraction.len()).ok()?;
    let mut value: i128 = format!("{whole}{fraction}").parse().ok()?;
    if shift >= 0 {
        value = value.checked_mul(10_i128.checked_pow(shift.unsigned_abs())?)?;
    } else {
        // More digits than the scale: Oracle rounds to the declared scale already.
        value /= 10_i128.checked_pow(shift.unsigned_abs())?;
    }
    Some(if negative { -value } else { value })
}

/// The builder of one column.
enum Builder {
    Int64(Int64Builder),
    Decimal(Decimal128Builder, i8),
    Float32(Float32Builder),
    Float64(Float64Builder),
    Text(StringBuilder),
    Binary(BinaryBuilder),
    Date(TimestampSecondBuilder),
    Timestamp(TimestampMicrosecondBuilder, bool),
    Boolean(BooleanBuilder),
}

fn value_error(e: &oracle::Error) -> Error {
    Error::Other(format!("cannot read a value: {e}"))
}

impl Builder {
    fn new(kind: Kind, capacity: usize) -> Result<Self> {
        Ok(match kind {
            Kind::Int64 => Self::Int64(Int64Builder::with_capacity(capacity)),
            Kind::Decimal(p, s) => Self::Decimal(
                Decimal128Builder::with_capacity(capacity).with_precision_and_scale(p, s)?,
                s,
            ),
            Kind::Float32 => Self::Float32(Float32Builder::with_capacity(capacity)),
            Kind::Float64 => Self::Float64(Float64Builder::with_capacity(capacity)),
            Kind::Text => Self::Text(StringBuilder::with_capacity(capacity, capacity * 16)),
            Kind::Binary => Self::Binary(BinaryBuilder::with_capacity(capacity, capacity * 16)),
            Kind::Date => Self::Date(TimestampSecondBuilder::with_capacity(capacity)),
            Kind::Timestamp => {
                Self::Timestamp(TimestampMicrosecondBuilder::with_capacity(capacity), false)
            }
            Kind::Zoned => Self::Timestamp(
                TimestampMicrosecondBuilder::with_capacity(capacity).with_timezone("+00:00"),
                true,
            ),
            Kind::Boolean => Self::Boolean(BooleanBuilder::with_capacity(capacity)),
        })
    }

    fn append(&mut self, value: &SqlValue) -> Result<()> {
        match self {
            Self::Int64(b) => {
                b.append_option(value.get::<Option<i64>>().map_err(|e| value_error(&e))?);
            }
            Self::Decimal(b, scale) => {
                let text: Option<String> = value.get().map_err(|e| value_error(&e))?;
                b.append_option(text.and_then(|t| decimal(&t, *scale)));
            }
            Self::Float32(b) => {
                b.append_option(value.get::<Option<f32>>().map_err(|e| value_error(&e))?);
            }
            Self::Float64(b) => {
                b.append_option(value.get::<Option<f64>>().map_err(|e| value_error(&e))?);
            }
            // Types without a text form (cursors, files) read as NULL.
            Self::Text(b) => b.append_option(value.get::<Option<String>>().ok().flatten()),
            Self::Binary(b) => {
                b.append_option(
                    value
                        .get::<Option<Vec<u8>>>()
                        .map_err(|e| value_error(&e))?,
                );
            }
            Self::Date(b) => {
                let t: Option<Timestamp> = value.get().map_err(|e| value_error(&e))?;
                b.append_option(t.map(|t| epoch_seconds(&t)));
            }
            Self::Timestamp(b, zoned) => {
                let t: Option<Timestamp> = value.get().map_err(|e| value_error(&e))?;
                b.append_option(t.map(|t| epoch_micros(&t, *zoned)));
            }
            Self::Boolean(b) => {
                b.append_option(value.get::<Option<bool>>().map_err(|e| value_error(&e))?);
            }
        }
        Ok(())
    }

    fn finish(&mut self) -> ArrayRef {
        match self {
            Self::Int64(b) => Arc::new(b.finish()),
            Self::Decimal(b, _) => Arc::new(b.finish()),
            Self::Float32(b) => Arc::new(b.finish()),
            Self::Float64(b) => Arc::new(b.finish()),
            Self::Text(b) => Arc::new(b.finish()),
            Self::Binary(b) => Arc::new(b.finish()),
            Self::Date(b) => Arc::new(b.finish()),
            Self::Timestamp(b, _) => Arc::new(b.finish()),
            Self::Boolean(b) => Arc::new(b.finish()),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Int64(b) => b.len(),
            Self::Decimal(b, _) => b.len(),
            Self::Float32(b) => b.len(),
            Self::Float64(b) => b.len(),
            Self::Text(b) => b.len(),
            Self::Binary(b) => b.len(),
            Self::Date(b) => b.len(),
            Self::Timestamp(b, _) => b.len(),
            Self::Boolean(b) => b.len(),
        }
    }
}

/// Collects rows into batches of a result's schema.
pub struct Batcher {
    schema: SchemaRef,
    builders: Vec<Builder>,
}

impl Batcher {
    pub fn new(columns: &[ColumnInfo], capacity: usize) -> Result<Self> {
        Ok(Self {
            schema: schema(columns),
            builders: columns
                .iter()
                .map(|c| Builder::new(kind(c.oracle_type()), capacity))
                .collect::<Result<_>>()?,
        })
    }

    #[must_use]
    pub fn schema(&self) -> SchemaRef {
        Arc::clone(&self.schema)
    }

    /// Appends a row's values, one per column.
    pub fn push(&mut self, values: &[SqlValue]) -> Result<()> {
        for (builder, value) in self.builders.iter_mut().zip(values) {
            builder.append(value)?;
        }
        Ok(())
    }

    #[must_use]
    pub fn rows(&self) -> usize {
        self.builders.first().map_or(0, Builder::len)
    }

    /// The rows pushed since the last batch.
    pub fn finish(&mut self) -> Result<RecordBatch> {
        let columns = self.builders.iter_mut().map(Builder::finish).collect();
        Ok(RecordBatch::try_new(Arc::clone(&self.schema), columns)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_of_oracle_types() {
        assert_eq!(kind(&OracleType::Number(10, 0)), Kind::Int64);
        assert_eq!(kind(&OracleType::Number(19, 0)), Kind::Text, "beyond i64");
        assert_eq!(kind(&OracleType::Number(5, 2)), Kind::Decimal(5, 2));
        assert_eq!(
            kind(&OracleType::Number(0, -127)),
            Kind::Text,
            "no precision"
        );
        assert_eq!(kind(&OracleType::Number(5, -2)), Kind::Text);
        assert_eq!(kind(&OracleType::Float(126)), Kind::Float64);
        assert_eq!(kind(&OracleType::BinaryFloat), Kind::Float32);
        assert_eq!(kind(&OracleType::Varchar2(20)), Kind::Text);
        assert_eq!(kind(&OracleType::CLOB), Kind::Text);
        assert_eq!(kind(&OracleType::BLOB), Kind::Binary);
        assert_eq!(kind(&OracleType::Date), Kind::Date);
        assert_eq!(kind(&OracleType::TimestampTZ(6)), Kind::Zoned);
        assert_eq!(kind(&OracleType::Boolean), Kind::Boolean);
    }

    #[test]
    fn decimals_from_text() {
        assert_eq!(decimal("2.5", 2), Some(250));
        assert_eq!(decimal("-12.25", 2), Some(-1225));
        assert_eq!(decimal(".5", 1), Some(5));
        assert_eq!(decimal("7", 3), Some(7000));
        assert_eq!(decimal("1E+3", 0), Some(1000));
        assert_eq!(decimal("1.5E-1", 2), Some(15));
        assert_eq!(decimal("x", 2), None);
    }

    #[test]
    fn calendar_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2024, 1, 2), 19_724);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
    }

    #[test]
    fn timestamps_to_utc() {
        let t = Timestamp::new(2024, 1, 2, 3, 4, 5, 6000).unwrap();
        assert_eq!(
            epoch_micros(&t, false),
            (19_724 * 86_400 + 3 * 3600 + 4 * 60 + 5) * 1_000_000 + 6
        );
        let zoned = t.and_tz_hm_offset(2, 0).unwrap();
        assert_eq!(
            epoch_micros(&zoned, true),
            epoch_micros(&t, false) - 7200 * 1_000_000
        );
    }
}
