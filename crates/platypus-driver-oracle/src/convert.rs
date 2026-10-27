//! Oracle values into Arrow arrays. Numbers with a declared precision keep it (integers,
//! decimals); a `NUMBER` without one can hold any of them, so its scale comes from the first
//! rows of the result (see [`number_scale`]). Times without a zone are naive timestamps;
//! `TIMESTAMP WITH [LOCAL] TIME ZONE` is converted to UTC. A fraction of more than six digits
//! is kept in nanoseconds when the first rows fit their range (1677 to 2262).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use arrow_array::RecordBatch;
use arrow_array::builder::{
    ArrayBuilder, BinaryBuilder, BooleanBuilder, Decimal128Builder, Float32Builder, Float64Builder,
    Int64Builder, StringBuilder, TimestampMicrosecondBuilder, TimestampNanosecondBuilder,
    TimestampSecondBuilder,
};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use oracle::sql_type::{FromSql, OracleType, RefCursor, Timestamp};
use oracle::{ColumnInfo, SqlValue};
use platypus_core::{Error, META_DB_TYPE, Result};

/// Digits of the widest decimal Arrow holds (`Decimal128`).
const MAX_DIGITS: u8 = 38;

/// Rows of a cursor expression shown in its cell, at most.
const CURSOR_ROWS: usize = 100;

/// How a column's values are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Int64,
    Decimal(u8, i8),
    /// `NUMBER` without a precision, or a `FLOAT` beyond what a double holds: a decimal of
    /// 38 digits whose scale the first rows decide.
    Number,
    Float32,
    Float64,
    Text,
    /// A cursor expression: its rows, as JSON.
    Cursor,
    Binary,
    /// `DATE`, which holds seconds.
    Date,
    /// `zoned` ones converted to UTC; `nanos` for a fraction of more than six digits.
    Timestamp {
        zoned: bool,
        nanos: bool,
    },
    Boolean,
}

/// The kind of an Oracle type.
#[must_use]
pub fn kind(t: &OracleType) -> Kind {
    match *t {
        OracleType::Number(p, 0) if (1..=18).contains(&p) => Kind::Int64,
        // No precision (0): a column `NUMBER`, or an expression (`LEVEL / 2` says scale 0
        // and holds 0.5), left to the first rows below.
        OracleType::Number(p @ 1..=38, s) if s >= 0 => {
            // Arrow wants the scale within the precision: `NUMBER(2,5)` holds 0.00099.
            Kind::Decimal(p.max(s.unsigned_abs()).min(MAX_DIGITS), s)
        }
        // `NUMBER(5,-2)`: whole numbers of up to seven digits.
        OracleType::Number(p @ 1..=38, s)
            if s < 0 && u16::from(p) + u16::from(s.unsigned_abs()) <= 38 =>
        {
            Kind::Decimal(p + s.unsigned_abs(), 0)
        }
        // `FLOAT(p)` is a decimal number of `p` bits: up to 49 bits (15 digits) a double
        // holds every value exactly, beyond (the default is 126) it is a decimal.
        OracleType::Float(p) if p <= 49 => Kind::Float64,
        OracleType::Number(..) | OracleType::Float(_) => Kind::Number,
        OracleType::Int64 => Kind::Int64,
        OracleType::BinaryFloat => Kind::Float32,
        OracleType::BinaryDouble => Kind::Float64,
        OracleType::RefCursor => Kind::Cursor,
        OracleType::Raw(_) | OracleType::LongRaw | OracleType::BLOB => Kind::Binary,
        OracleType::Date => Kind::Date,
        OracleType::Timestamp(p) => Kind::Timestamp {
            zoned: false,
            nanos: p > 6,
        },
        OracleType::TimestampTZ(p) | OracleType::TimestampLTZ(p) => Kind::Timestamp {
            zoned: true,
            nanos: p > 6,
        },
        OracleType::Boolean => Kind::Boolean,
        _ => Kind::Text,
    }
}

/// The Arrow type of a column of `kind`; for a [`Kind::Number`] or a nanosecond timestamp,
/// the type until the first rows decide.
fn data_type(kind: Kind) -> DataType {
    match kind {
        Kind::Int64 => DataType::Int64,
        Kind::Decimal(p, s) => DataType::Decimal128(p, s),
        Kind::Number => DataType::Decimal128(MAX_DIGITS, 0),
        Kind::Float32 => DataType::Float32,
        Kind::Float64 => DataType::Float64,
        Kind::Text | Kind::Cursor => DataType::Utf8,
        Kind::Binary => DataType::Binary,
        Kind::Date => DataType::Timestamp(TimeUnit::Second, None),
        Kind::Timestamp { zoned, nanos } => timestamp_type(zoned, nanos),
        Kind::Boolean => DataType::Boolean,
    }
}

fn timestamp_type(zoned: bool, nanos: bool) -> DataType {
    let unit = if nanos {
        TimeUnit::Nanosecond
    } else {
        TimeUnit::Microsecond
    };
    DataType::Timestamp(unit, zoned.then(|| "+00:00".into()))
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

/// Seconds since the epoch; in UTC when `zoned`.
fn zoned_seconds(t: &Timestamp, zoned: bool) -> i64 {
    let seconds = epoch_seconds(t);
    if zoned {
        seconds - i64::from(t.tz_offset())
    } else {
        seconds
    }
}

/// Microseconds since the epoch; in UTC when `zoned`.
#[must_use]
pub fn epoch_micros(t: &Timestamp, zoned: bool) -> i64 {
    zoned_seconds(t, zoned) * 1_000_000 + i64::from(t.nanosecond() / 1000)
}

/// Nanoseconds since the epoch, in UTC when `zoned`; `None` outside 1677 to 2262.
#[must_use]
pub fn epoch_nanos(t: &Timestamp, zoned: bool) -> Option<i64> {
    zoned_seconds(t, zoned)
        .checked_mul(1_000_000_000)?
        .checked_add(i64::from(t.nanosecond()))
}

/// The decimal `text` (`-12.5`, `.5`, `1E+3`) as an integer of `scale` decimal places,
/// rounded half away from zero; `None` when it does not fit.
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
    let digits = format!("{whole}{fraction}");
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: i128 = if shift >= 0 {
        digits
            .parse::<i128>()
            .ok()?
            .checked_mul(10_i128.checked_pow(shift.unsigned_abs())?)?
    } else {
        // The digits beyond the scale go, the first of them rounding the rest.
        let cut = usize::try_from(shift.unsigned_abs()).ok()?;
        match digits.len().checked_sub(cut) {
            Some(keep) => {
                let kept = if keep == 0 {
                    0
                } else {
                    digits[..keep].parse().ok()?
                };
                kept + i128::from(digits.as_bytes()[keep] >= b'5')
            }
            None => 0,
        }
    };
    Some(if negative { -value } else { value })
}

/// The digits of the decimal `text` before and after its point, without the zeros that do
/// not count (`0012.500` has 2 and 1); `None` when it is not a number.
fn digit_counts(text: &str) -> Option<(usize, usize)> {
    let text = text.trim();
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(i) => (&text[..i], text[i + 1..].parse::<i64>().ok()?),
        None => (text, 0),
    };
    let digits = mantissa.trim_start_matches(['-', '+']);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if (whole.is_empty() && fraction.is_empty())
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let all = format!("{whole}{fraction}");
    let leading = i64::try_from(all.len() - all.trim_start_matches('0').len()).ok()?;
    let end = i64::try_from(all.trim_end_matches('0').len()).ok()?;
    if end == 0 {
        return Some((0, 0));
    }
    // Where the point stands among the digits.
    let point = i64::try_from(whole.len()).ok()? + exponent;
    let count = |n: i64| usize::try_from(n.max(0)).ok();
    Some((count(point - leading)?, count(end - point)?))
}

/// The scale of a decimal of 38 digits that holds the numbers `values` (decimal text): every
/// whole digit, and as many digits after the point as the widest has and room is left for
/// (a `NUMBER` has 38 significant digits, rarely 40). `None` when one is not a number, or
/// has more than 38 whole digits.
#[must_use]
pub fn number_scale<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<i8> {
    let (mut whole, mut fraction) = (0, 0);
    for value in values {
        let (w, f) = digit_counts(value)?;
        whole = whole.max(w);
        fraction = fraction.max(f);
    }
    let room = usize::from(MAX_DIGITS).checked_sub(whole)?;
    i8::try_from(fraction.min(room)).ok()
}

/// The largest value of a decimal of 38 digits, plus one.
const DECIMAL_LIMIT: i128 = 10_i128.pow(38);

/// The value as `T`.
fn get<T: FromSql>(value: &SqlValue) -> Result<T> {
    value
        .get()
        .map_err(|e| Error::Other(format!("cannot read a value: {e}")))
}

/// A JSON string of `text` into `out`.
fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The rows of a cursor expression as a JSON array of objects, their values as text, up to
/// [`CURSOR_ROWS`] of them.
fn cursor_text(mut cursor: RefCursor) -> Result<String> {
    let rows = cursor
        .query()
        .map_err(|e| Error::Other(format!("cannot read a cursor: {e}")))?;
    let names: Vec<String> = rows
        .column_info()
        .iter()
        .map(|c| c.name().to_owned())
        .collect();
    let mut out = String::from("[");
    for (n, row) in rows.take(CURSOR_ROWS).enumerate() {
        let row = row.map_err(|e| Error::Other(format!("cannot read a cursor: {e}")))?;
        out.push_str(if n == 0 { "{" } else { ",{" });
        for (i, (name, value)) in names.iter().zip(row.sql_values()).enumerate() {
            if i > 0 {
                out.push(',');
            }
            json_string(&mut out, name);
            out.push(':');
            // Nested cursors and files have no text: null.
            match value.get::<Option<String>>().ok().flatten() {
                Some(text) => json_string(&mut out, &text),
                None => out.push_str("null"),
            }
        }
        out.push('}');
    }
    out.push(']');
    Ok(out)
}

/// The builder of one column.
enum Builder {
    Int64(Int64Builder),
    /// With its scale, and whether the scale came from the first rows (a later value that
    /// does not fit is an error then, not a NULL).
    Decimal(Decimal128Builder, i8, bool),
    Float32(Float32Builder),
    Float64(Float64Builder),
    Text(StringBuilder),
    Cursor(StringBuilder),
    Binary(BinaryBuilder),
    Date(TimestampSecondBuilder),
    Timestamp(TimestampMicrosecondBuilder, bool),
    Nanos(TimestampNanosecondBuilder, bool),
    Boolean(BooleanBuilder),
    /// The values of a [`Kind::Number`], until the first rows decide its scale.
    PendingNumber(Vec<Option<String>>),
    /// The values of a nanosecond timestamp (zoned or not), until the first rows tell
    /// whether nanoseconds reach them.
    PendingNanos(Vec<Option<Timestamp>>, bool),
}

impl Builder {
    fn new(kind: Kind, capacity: usize) -> Result<Self> {
        Ok(match kind {
            Kind::Int64 => Self::Int64(Int64Builder::with_capacity(capacity)),
            Kind::Decimal(p, s) => Self::decimal(p, s, false, capacity)?,
            Kind::Number => Self::PendingNumber(Vec::with_capacity(capacity)),
            Kind::Float32 => Self::Float32(Float32Builder::with_capacity(capacity)),
            Kind::Float64 => Self::Float64(Float64Builder::with_capacity(capacity)),
            Kind::Text => Self::Text(Self::strings(capacity)),
            Kind::Cursor => Self::Cursor(Self::strings(capacity)),
            Kind::Binary => Self::Binary(BinaryBuilder::with_capacity(capacity, capacity * 16)),
            Kind::Date => Self::Date(TimestampSecondBuilder::with_capacity(capacity)),
            Kind::Timestamp { zoned, nanos: true } => {
                Self::PendingNanos(Vec::with_capacity(capacity), zoned)
            }
            Kind::Timestamp {
                zoned,
                nanos: false,
            } => Self::micros(zoned, capacity),
            Kind::Boolean => Self::Boolean(BooleanBuilder::with_capacity(capacity)),
        })
    }

    fn strings(capacity: usize) -> StringBuilder {
        StringBuilder::with_capacity(capacity, capacity * 16)
    }

    fn decimal(precision: u8, scale: i8, decided: bool, capacity: usize) -> Result<Self> {
        Ok(Self::Decimal(
            Decimal128Builder::with_capacity(capacity)
                .with_precision_and_scale(precision, scale)?,
            scale,
            decided,
        ))
    }

    fn micros(zoned: bool, capacity: usize) -> Self {
        let builder = TimestampMicrosecondBuilder::with_capacity(capacity);
        Self::Timestamp(
            if zoned {
                builder.with_timezone("+00:00")
            } else {
                builder
            },
            zoned,
        )
    }

    fn nanos(zoned: bool, capacity: usize) -> Self {
        let builder = TimestampNanosecondBuilder::with_capacity(capacity);
        Self::Nanos(
            if zoned {
                builder.with_timezone("+00:00")
            } else {
                builder
            },
            zoned,
        )
    }

    fn append(&mut self, value: &SqlValue) -> Result<()> {
        match self {
            Self::Int64(b) => b.append_option(get(value)?),
            Self::Decimal(b, scale, decided) => {
                let text: Option<String> = get(value)?;
                Self::append_decimal(b, *scale, *decided, text.as_deref())?;
            }
            Self::Float32(b) => b.append_option(get(value)?),
            Self::Float64(b) => b.append_option(get(value)?),
            Self::Text(b) => b.append_option(get::<Option<String>>(value)?),
            Self::Cursor(b) => match get::<Option<RefCursor>>(value)? {
                Some(cursor) => b.append_value(cursor_text(cursor)?),
                None => b.append_null(),
            },
            Self::Binary(b) => b.append_option(get::<Option<Vec<u8>>>(value)?),
            Self::Date(b) => {
                let t: Option<Timestamp> = get(value)?;
                b.append_option(t.map(|t| epoch_seconds(&t)));
            }
            Self::Timestamp(b, zoned) => {
                let t: Option<Timestamp> = get(value)?;
                b.append_option(t.map(|t| epoch_micros(&t, *zoned)));
            }
            Self::Nanos(b, zoned) => {
                let t: Option<Timestamp> = get(value)?;
                b.append_option(t.map(|t| nanos_of(&t, *zoned)).transpose()?);
            }
            Self::Boolean(b) => b.append_option(get(value)?),
            Self::PendingNumber(values) => values.push(get(value)?),
            Self::PendingNanos(values, _) => values.push(get(value)?),
        }
        Ok(())
    }

    /// A declared decimal reads a value it cannot hold as NULL; one whose scale the first
    /// rows decided fails the result, which a NULL would misreport.
    fn append_decimal(
        b: &mut Decimal128Builder,
        scale: i8,
        decided: bool,
        text: Option<&str>,
    ) -> Result<()> {
        let Some(text) = text else {
            b.append_null();
            return Ok(());
        };
        match decimal(text, scale).filter(|v| v.unsigned_abs() < DECIMAL_LIMIT.unsigned_abs()) {
            Some(v) => b.append_value(v),
            None if decided => {
                return Err(Error::Other(format!(
                    "the number {text} does not fit the column's DECIMAL(38,{scale}), which \
                     its first rows set: convert the column with TO_CHAR or CAST"
                )));
            }
            None => b.append_null(),
        }
        Ok(())
    }

    /// Settles a pending column with the values it holds: the Arrow type it takes, `None`
    /// when it was not pending.
    fn settle(&mut self) -> Result<Option<DataType>> {
        let settled = match self {
            Self::PendingNumber(values) => {
                let capacity = values.capacity();
                let texts = values.iter().flatten().map(String::as_str);
                if let Some(scale) = number_scale(texts) {
                    let mut b = Self::decimal(MAX_DIGITS, scale, true, capacity)?;
                    if let Self::Decimal(d, ..) = &mut b {
                        for v in values.iter() {
                            Self::append_decimal(d, scale, true, v.as_deref())?;
                        }
                    }
                    b
                } else {
                    // Beyond any decimal: the exact text.
                    let mut b = Self::strings(capacity);
                    for v in values.iter() {
                        b.append_option(v.as_deref());
                    }
                    Self::Text(b)
                }
            }
            Self::PendingNanos(values, zoned) => {
                let zoned = *zoned;
                let capacity = values.capacity();
                let fits = values
                    .iter()
                    .flatten()
                    .all(|t| epoch_nanos(t, zoned).is_some());
                let mut b = if fits {
                    Self::nanos(zoned, capacity)
                } else {
                    Self::micros(zoned, capacity)
                };
                for t in values.iter() {
                    match &mut b {
                        Self::Nanos(n, _) => {
                            n.append_option(t.as_ref().and_then(|t| epoch_nanos(t, zoned)));
                        }
                        Self::Timestamp(m, _) => {
                            m.append_option(t.as_ref().map(|t| epoch_micros(t, zoned)));
                        }
                        _ => {}
                    }
                }
                b
            }
            _ => return Ok(None),
        };
        *self = settled;
        Ok(Some(self.data_type()))
    }

    /// The Arrow type of a settled column.
    fn data_type(&self) -> DataType {
        match self {
            Self::Decimal(_, scale, _) => DataType::Decimal128(MAX_DIGITS, *scale),
            Self::Nanos(_, zoned) => timestamp_type(*zoned, true),
            Self::Timestamp(_, zoned) => timestamp_type(*zoned, false),
            _ => DataType::Utf8,
        }
    }

    /// The Arrow builder inside, for what all builders do alike.
    fn array_builder(&mut self) -> &mut dyn ArrayBuilder {
        match self {
            Self::Int64(b) => b,
            Self::Decimal(b, ..) => b,
            Self::Float32(b) => b,
            Self::Float64(b) => b,
            Self::Text(b) | Self::Cursor(b) => b,
            Self::Binary(b) => b,
            Self::Date(b) => b,
            Self::Timestamp(b, _) => b,
            Self::Nanos(b, _) => b,
            Self::Boolean(b) => b,
            Self::PendingNumber(_) | Self::PendingNanos(..) => {
                unreachable!("a pending column is settled before it is built")
            }
        }
    }

    /// Values appended since the last batch.
    fn len(&self) -> usize {
        match self {
            Self::PendingNumber(values) => values.len(),
            Self::PendingNanos(values, _) => values.len(),
            Self::Int64(b) => b.len(),
            Self::Decimal(b, ..) => b.len(),
            Self::Float32(b) => b.len(),
            Self::Float64(b) => b.len(),
            Self::Text(b) | Self::Cursor(b) => b.len(),
            Self::Binary(b) => b.len(),
            Self::Date(b) => b.len(),
            Self::Timestamp(b, _) => b.len(),
            Self::Nanos(b, _) => b.len(),
            Self::Boolean(b) => b.len(),
        }
    }
}

/// Nanoseconds since the epoch, or the error of a time beyond their range.
fn nanos_of(t: &Timestamp, zoned: bool) -> Result<i64> {
    epoch_nanos(t, zoned).ok_or_else(|| {
        Error::Other(format!(
            "the time {t} is outside 1677 to 2262, where the column's first rows kept \
             nanoseconds: convert the column with TO_CHAR or CAST"
        ))
    })
}

/// Collects rows into batches of a result's schema.
pub struct Batcher {
    schema: SchemaRef,
    builders: Vec<Builder>,
}

impl Batcher {
    pub fn new(columns: &[ColumnInfo], capacity: usize) -> Result<Self> {
        let fields: Vec<Field> = columns
            .iter()
            .map(|c| {
                Field::new(c.name(), data_type(kind(c.oracle_type())), c.nullable()).with_metadata(
                    HashMap::from([(META_DB_TYPE.to_owned(), c.oracle_type().to_string())]),
                )
            })
            .collect();
        Ok(Self {
            schema: Arc::new(Schema::new(fields)),
            builders: columns
                .iter()
                .map(|c| Builder::new(kind(c.oracle_type()), capacity))
                .collect::<Result<_>>()?,
        })
    }

    /// Names the database type of column `index` as `label`, for a column read through a
    /// conversion (a `JSON` column read as its text).
    pub fn label(&mut self, index: usize, label: &str) {
        let mut fields: Vec<Field> = self
            .schema
            .fields()
            .iter()
            .map(|f| f.as_ref().clone())
            .collect();
        if let Some(field) = fields.get_mut(index) {
            let mut metadata = field.metadata().clone();
            metadata.insert(META_DB_TYPE.to_owned(), label.to_owned());
            *field = field.clone().with_metadata(metadata);
        }
        self.schema = Arc::new(Schema::new(fields));
    }

    /// Settles the columns whose type the first rows decide, with the rows pushed so far.
    fn settle(&mut self) -> Result<()> {
        let mut fields: Option<Vec<Field>> = None;
        for (i, builder) in self.builders.iter_mut().enumerate() {
            if let Some(t) = builder.settle()? {
                let fields = fields.get_or_insert_with(|| {
                    self.schema
                        .fields()
                        .iter()
                        .map(|f| f.as_ref().clone())
                        .collect()
                });
                fields[i] = fields[i].clone().with_data_type(t);
            }
        }
        if let Some(fields) = fields {
            self.schema = Arc::new(Schema::new(fields));
        }
        Ok(())
    }

    /// The schema of the batches, once the first rows have settled it.
    pub fn schema(&mut self) -> Result<SchemaRef> {
        self.settle()?;
        Ok(Arc::clone(&self.schema))
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
        self.settle()?;
        let columns = self
            .builders
            .iter_mut()
            .map(|b| b.array_builder().finish())
            .collect();
        Ok(RecordBatch::try_new(Arc::clone(&self.schema), columns)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_of_oracle_types() {
        assert_eq!(kind(&OracleType::Number(10, 0)), Kind::Int64);
        assert_eq!(
            kind(&OracleType::Number(38, 0)),
            Kind::Decimal(38, 0),
            "INTEGER"
        );
        assert_eq!(kind(&OracleType::Number(5, 2)), Kind::Decimal(5, 2));
        assert_eq!(kind(&OracleType::Number(2, 5)), Kind::Decimal(5, 5));
        assert_eq!(
            kind(&OracleType::Number(0, 0)),
            Kind::Number,
            "an expression"
        );
        assert_eq!(
            kind(&OracleType::Number(0, -127)),
            Kind::Number,
            "no precision"
        );
        assert_eq!(kind(&OracleType::Number(5, -2)), Kind::Decimal(7, 0));
        assert_eq!(kind(&OracleType::Number(38, -2)), Kind::Number);
        assert_eq!(kind(&OracleType::Float(126)), Kind::Number, "38 digits");
        assert_eq!(kind(&OracleType::Float(49)), Kind::Float64, "15 digits");
        assert_eq!(kind(&OracleType::RefCursor), Kind::Cursor);
        assert_eq!(
            kind(&OracleType::BFILE),
            Kind::Text,
            "read through the query"
        );
        assert_eq!(kind(&OracleType::BinaryFloat), Kind::Float32);
        assert_eq!(kind(&OracleType::Varchar2(20)), Kind::Text);
        assert_eq!(kind(&OracleType::CLOB), Kind::Text);
        assert_eq!(kind(&OracleType::BLOB), Kind::Binary);
        assert_eq!(kind(&OracleType::Date), Kind::Date);
        assert_eq!(
            kind(&OracleType::Timestamp(9)),
            Kind::Timestamp {
                zoned: false,
                nanos: true
            }
        );
        assert_eq!(
            kind(&OracleType::TimestampTZ(6)),
            Kind::Timestamp {
                zoned: true,
                nanos: false
            }
        );
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
        // Rounded half away from zero.
        assert_eq!(decimal("0.125", 2), Some(13));
        assert_eq!(decimal("-0.125", 2), Some(-13));
        assert_eq!(decimal("0.124", 2), Some(12));
        assert_eq!(decimal("1E-200", 2), Some(0));
    }

    #[test]
    fn scales_from_the_first_rows() {
        assert_eq!(number_scale(["1", "20", "-3"]), Some(0));
        assert_eq!(number_scale(["12.5", "3", "0.25"]), Some(2));
        assert_eq!(number_scale(["1.50", "100"]), Some(1), "trailing zeros");
        assert_eq!(number_scale(["1E+3", ".001"]), Some(3));
        assert_eq!(number_scale([] as [&str; 0]), Some(0));
        // 1/3 has 40 digits: 38 are kept.
        let third = format!(".{}", "3".repeat(40));
        assert_eq!(number_scale([third.as_str()]), Some(38));
        assert_eq!(
            number_scale(["6461.83177570093457943925233644859813084"]),
            Some(34)
        );
        assert_eq!(number_scale(["1E+125"]), None, "beyond 38 digits");
        assert_eq!(number_scale(["abc"]), None);
        assert_eq!(digit_counts("0012.500"), Some((2, 1)));
        assert_eq!(digit_counts("0"), Some((0, 0)));
        assert_eq!(digit_counts("-1.5E-1"), Some((0, 2)));
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
        let t = Timestamp::new(2024, 1, 2, 3, 4, 5, 6789).unwrap();
        let seconds = 19_724 * 86_400 + 3 * 3600 + 4 * 60 + 5;
        assert_eq!(epoch_micros(&t, false), seconds * 1_000_000 + 6);
        assert_eq!(epoch_nanos(&t, false), Some(seconds * 1_000_000_000 + 6789));
        let zoned = t.and_tz_hm_offset(2, 0).unwrap();
        assert_eq!(
            epoch_micros(&zoned, true),
            epoch_micros(&t, false) - 7200 * 1_000_000
        );
        let far = Timestamp::new(9999, 12, 31, 0, 0, 0, 0).unwrap();
        assert_eq!(epoch_nanos(&far, false), None);
    }

    #[test]
    fn cursors_as_json_strings() {
        let mut out = String::new();
        json_string(&mut out, "a\"b\\c\nd\u{1}");
        assert_eq!(out, "\"a\\\"b\\\\c\\nd\\u0001\"");
    }
}
