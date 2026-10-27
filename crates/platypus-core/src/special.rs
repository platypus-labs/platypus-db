//! Values that a typed Arrow column cannot hold, but some databases store: `±infinity` dates
//! and timestamps (PostgreSQL), dates outside the calendar such as MySQL's `0000-00-00`, and
//! `NaN` in a PostgreSQL `numeric(p,s)`.
//!
//! Drivers keep the column's type and store these values as reserved numbers that no real
//! value uses: outside the dates chrono can show, or above the largest decimal of 38 digits.
//! [`column_formatter`] shows them as the text the database prints and reads back
//! (`infinity`, `0000-00-00`, `NaN`); every place that turns a result's values into text goes
//! through it. The infinities are DuckDB's own, so DuckDB and Parquet readers see them as
//! infinities too.
//!
//! The decimal `NaN` is the one reserved value another source can hold as a real number: a
//! DuckDB HUGEINT read as `Decimal128(38, 0)` reaches `i128::MAX`. So it is a `NaN` only in a
//! field the driver marks with [`META_DECIMAL_NAN`] (PostgreSQL's `numeric(p,s)`, whose real
//! values have at most 38 digits); everywhere else it is the number it is.

use std::fmt::Write;

use arrow_array::cast::AsArray;
use arrow_array::types::{
    Date32Type, Decimal128Type, TimestampMicrosecondType, TimestampMillisecondType,
    TimestampSecondType,
};
use arrow_array::{Array, ArrayRef, RecordBatch, StringArray};
use arrow_cast::display::{ArrayFormatter, DisplayIndex, FormatOptions, FormatResult};
use arrow_schema::{ArrowError, DataType, Field, Schema, TimeUnit};

/// `Date32` days of `infinity` and `-infinity`.
pub const DATE_INFINITY: i32 = i32::MAX;
pub const DATE_NEG_INFINITY: i32 = -i32::MAX;
/// Timestamps (seconds, milliseconds or microseconds) of `infinity` and `-infinity`.
pub const TIMESTAMP_INFINITY: i64 = i64::MAX;
pub const TIMESTAMP_NEG_INFINITY: i64 = -i64::MAX;
/// A `Decimal128` `NaN`: above the largest value of 38 digits. Only in a field marked with
/// [`META_DECIMAL_NAN`].
pub const DECIMAL_NAN: i128 = i128::MAX;
/// Field metadata of a `Decimal128` column whose [`DECIMAL_NAN`] values are `NaN`.
pub const META_DECIMAL_NAN: &str = "platypus.decimal_nan";

/// `field` marked as holding [`DECIMAL_NAN`] for `NaN`.
#[must_use]
pub fn with_decimal_nan(field: Field) -> Field {
    let mut metadata = field.metadata().clone();
    metadata.insert(META_DECIMAL_NAN.to_owned(), "true".to_owned());
    field.with_metadata(metadata)
}

/// A `Decimal128` column whose [`DECIMAL_NAN`] values are `NaN`.
#[must_use]
pub fn holds_decimal_nan(field: &Field) -> bool {
    matches!(field.data_type(), DataType::Decimal128(..))
        && field.metadata().contains_key(META_DECIMAL_NAN)
}

/// Column `array` of `field` with its decimal `NaN` values as NULL, for sums and extremes,
/// which leave `NaN` out; any other column as it is.
#[must_use]
pub fn without_decimal_nan(field: &Field, array: &ArrayRef) -> ArrayRef {
    if !holds_decimal_nan(field) {
        return std::sync::Arc::clone(array);
    }
    let decimals = array.as_primitive::<Decimal128Type>();
    let numbers: arrow_array::Decimal128Array = decimals
        .iter()
        .map(|v| v.filter(|&v| v != DECIMAL_NAN))
        .collect();
    let DataType::Decimal128(precision, scale) = *array.data_type() else {
        return std::sync::Arc::clone(array);
    };
    numbers
        .with_precision_and_scale(precision, scale)
        .map_or_else(
            |_| std::sync::Arc::clone(array),
            |n| std::sync::Arc::new(n) as ArrayRef,
        )
}

/// Value `i` of column `array` of `field` is a `NaN`.
#[must_use]
pub fn is_decimal_nan(field: &Field, array: &dyn Array, i: usize) -> bool {
    holds_decimal_nan(field)
        && array.is_valid(i)
        && array.as_primitive::<Decimal128Type>().value(i) == DECIMAL_NAN
}

/// Dates outside the calendar are this many days plus `yyyymmdd`: about five million years
/// before the epoch, far outside chrono's range.
const INVALID_DATE_BASE: i32 = -2_000_000_000;
const INVALID_DATE_CODES: i32 = 100_000_000;

/// Microsecond timestamps outside the calendar are this plus a day code (see
/// [`invalid_timestamp`]) times a day, plus the time of day.
const INVALID_TIMESTAMP_BASE: i64 = -9_000_000_000_000_000_000;
const MICROS_PER_DAY: i64 = 86_400_000_000;
/// Day codes: `(year * 13 + month) * 32 + day`, for years up to 9999.
const INVALID_DAY_CODES: i64 = (9_999 * 13 + 13) * 32;

/// The `Date32` of a date the calendar does not have, such as `0000-00-00` or `2024-02-31`;
/// `None` past year 9999 or for a month above 12 or a day above 31.
#[must_use]
pub fn invalid_date(year: u16, month: u8, day: u8) -> Option<i32> {
    (year <= 9_999 && month <= 12 && day <= 31).then(|| {
        INVALID_DATE_BASE + i32::from(year) * 10_000 + i32::from(month) * 100 + i32::from(day)
    })
}

/// The microsecond timestamp of a date the calendar does not have, at `micros` after
/// midnight (less than a day); `None` as for [`invalid_date`].
#[must_use]
pub fn invalid_timestamp(year: u16, month: u8, day: u8, micros: i64) -> Option<i64> {
    let ok = year <= 9_999 && month <= 12 && day <= 31 && (0..MICROS_PER_DAY).contains(&micros);
    ok.then(|| {
        let code = (i64::from(year) * 13 + i64::from(month)) * 32 + i64::from(day);
        INVALID_TIMESTAMP_BASE + code * MICROS_PER_DAY + micros
    })
}

fn date_text(days: i32) -> Option<String> {
    match days {
        DATE_INFINITY => Some("infinity".to_owned()),
        DATE_NEG_INFINITY | i32::MIN => Some("-infinity".to_owned()),
        d if (INVALID_DATE_BASE..INVALID_DATE_BASE + INVALID_DATE_CODES).contains(&d) => {
            let code = d - INVALID_DATE_BASE;
            Some(format!(
                "{:04}-{:02}-{:02}",
                code / 10_000,
                code / 100 % 100,
                code % 100
            ))
        }
        _ => None,
    }
}

fn timestamp_text(value: i64, unit: TimeUnit) -> Option<String> {
    // Nanoseconds reach only 1677 to 2262: their extremes are real times.
    if unit == TimeUnit::Nanosecond {
        return None;
    }
    match value {
        TIMESTAMP_INFINITY => Some("infinity".to_owned()),
        TIMESTAMP_NEG_INFINITY | i64::MIN => Some("-infinity".to_owned()),
        v if unit == TimeUnit::Microsecond
            && (INVALID_TIMESTAMP_BASE
                ..INVALID_TIMESTAMP_BASE + INVALID_DAY_CODES * MICROS_PER_DAY)
                .contains(&v) =>
        {
            let offset = v - INVALID_TIMESTAMP_BASE;
            let (code, micros) = (offset / MICROS_PER_DAY, offset % MICROS_PER_DAY);
            let (day, month, year) = (code % 32, code / 32 % 13, code / 32 / 13);
            let seconds = micros / 1_000_000;
            let mut text = format!(
                "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
                seconds / 3_600,
                seconds / 60 % 60,
                seconds % 60
            );
            if micros % 1_000_000 != 0 {
                let _ = write!(text, ".{:06}", micros % 1_000_000);
            }
            Some(text)
        }
        _ => None,
    }
}

/// The text of value `i` of column `array` of `field` when it is one of the reserved values;
/// `None` for any other value, NULL included.
#[must_use]
pub fn column_text(field: &Field, array: &dyn Array, i: usize) -> Option<String> {
    if is_decimal_nan(field, array, i) {
        return Some("NaN".to_owned());
    }
    text(array, i)
}

/// The text of value `i` of `array` when it is one of the reserved dates or timestamps;
/// `None` for any other value, NULL included. A decimal `NaN` needs its field:
/// [`column_text`].
#[must_use]
pub fn text(array: &dyn Array, i: usize) -> Option<String> {
    if array.is_null(i) {
        return None;
    }
    match array.data_type() {
        DataType::Date32 => date_text(array.as_primitive::<Date32Type>().value(i)),
        DataType::Timestamp(unit, _) => {
            let value = match unit {
                TimeUnit::Second => array.as_primitive::<TimestampSecondType>().value(i),
                TimeUnit::Millisecond => array.as_primitive::<TimestampMillisecondType>().value(i),
                TimeUnit::Microsecond => array.as_primitive::<TimestampMicrosecondType>().value(i),
                TimeUnit::Nanosecond => return None,
            };
            timestamp_text(value, *unit)
        }
        _ => None,
    }
}

/// Whether arrays of this type can hold reserved dates or timestamps.
fn may_hold(data_type: &DataType) -> bool {
    matches!(data_type, DataType::Date32 | DataType::Timestamp(..))
}

/// The first reserved value of column `array` of `field`, with its row.
#[must_use]
pub fn find(field: &Field, array: &dyn Array) -> Option<(usize, String)> {
    if !may_hold(array.data_type()) && !holds_decimal_nan(field) {
        return None;
    }
    (0..array.len()).find_map(|i| column_text(field, array, i).map(|t| (i, t)))
}

struct Special<'a> {
    array: &'a dyn Array,
    inner: ArrayFormatter<'a>,
    /// The column's `DECIMAL_NAN` values are `NaN`.
    nan: bool,
}

impl DisplayIndex for Special<'_> {
    fn write(&self, idx: usize, f: &mut dyn Write) -> FormatResult {
        let nan = self.nan
            && self.array.is_valid(idx)
            && self.array.as_primitive::<Decimal128Type>().value(idx) == DECIMAL_NAN;
        if nan {
            f.write_str("NaN")?;
        } else if let Some(text) = text(self.array, idx) {
            f.write_str(&text)?;
        } else if before_christ(self.array, idx) {
            let shown = self.inner.value(idx).try_to_string()?;
            f.write_str(&bc_text(&shown).unwrap_or(shown))?;
        } else {
            self.inner.value(idx).write(f)?;
        }
        Ok(())
    }
}

/// Days from the epoch to 0001-01-01, and seconds: anything earlier is BC.
const FIRST_AD_DAY: i32 = -719_162;
const FIRST_AD_SECOND: i64 = -62_135_596_800;

/// Whether value `i` of `array` is a date or time before the year 1.
fn before_christ(array: &dyn Array, i: usize) -> bool {
    if array.is_null(i) {
        return false;
    }
    match array.data_type() {
        DataType::Date32 => array.as_primitive::<Date32Type>().value(i) < FIRST_AD_DAY,
        DataType::Timestamp(TimeUnit::Second, _) => {
            array.as_primitive::<TimestampSecondType>().value(i) < FIRST_AD_SECOND
        }
        DataType::Timestamp(TimeUnit::Millisecond, _) => {
            array.as_primitive::<TimestampMillisecondType>().value(i) < FIRST_AD_SECOND * 1_000
        }
        DataType::Timestamp(TimeUnit::Microsecond, _) => {
            array.as_primitive::<TimestampMicrosecondType>().value(i) < FIRST_AD_SECOND * 1_000_000
        }
        _ => false,
    }
}

/// A date shown with a year of 0 or less (`-0043-03-15`, as chrono counts) written as
/// PostgreSQL does: `0044-03-15 BC`, the rest of the text kept.
fn bc_text(text: &str) -> Option<String> {
    let (negative, rest) = text.strip_prefix('-').map_or((false, text), |r| (true, r));
    let digits = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let year: i64 = rest[..digits].parse().ok()?;
    let year = if negative { -year } else { year };
    (year <= 0).then(|| format!("{:04}{} BC", 1 - year, &rest[digits..]))
}

/// [`ArrayFormatter::try_new`] for column `array` of `field`, with the reserved values shown
/// as their text, the decimal `NaN` included.
///
/// # Errors
/// When Arrow cannot format arrays of this type.
pub fn column_formatter<'a>(
    field: &Field,
    array: &'a dyn Array,
    options: &FormatOptions<'a>,
) -> Result<ArrayFormatter<'a>, ArrowError> {
    if !holds_decimal_nan(field) {
        return formatter(array, options);
    }
    let inner = ArrayFormatter::try_new(array, options)?;
    Ok(ArrayFormatter::new(
        Box::new(Special {
            array,
            inner,
            nan: true,
        }),
        options.safe(),
    ))
}

/// [`ArrayFormatter::try_new`], with the reserved dates and timestamps shown as their text;
/// for a result's column, [`column_formatter`], which also knows its decimal `NaN`.
///
/// # Errors
/// When Arrow cannot format arrays of this type.
pub fn formatter<'a>(
    array: &'a dyn Array,
    options: &FormatOptions<'a>,
) -> Result<ArrayFormatter<'a>, ArrowError> {
    if matches!(
        array.data_type(),
        DataType::List(_)
            | DataType::LargeList(_)
            | DataType::FixedSizeList(..)
            | DataType::Struct(_)
    ) {
        let top = Top {
            nested: Nested::new(array, options)?,
            null: options.null(),
        };
        return Ok(ArrayFormatter::new(Box::new(top), options.safe()));
    }
    if matches!(array.data_type(), DataType::Map(..)) {
        // A NULL inside a map, where nothing would tell it from an empty text.
        let nested = options.clone().with_null("NULL");
        return ArrayFormatter::try_new(array, &nested);
    }
    let inner = ArrayFormatter::try_new(array, options)?;
    if !may_hold(array.data_type()) {
        return Ok(inner);
    }
    Ok(ArrayFormatter::new(
        Box::new(Special {
            array,
            inner,
            nan: false,
        }),
        options.safe(),
    ))
}

/// A list or struct written with its elements as literals, as DuckDB prints them:
/// `[1, NULL, 3]`, `['a', '', 'b']`, `{'a': 1, 'b': 'x'}`. Texts are quoted (a quote doubled),
/// so an empty text and NULL differ, and the value reads back as the literal it shows.
enum Nested<'a> {
    List {
        array: &'a dyn Array,
        values: Box<Nested<'a>>,
    },
    Struct {
        array: &'a dyn Array,
        names: Vec<String>,
        fields: Vec<Nested<'a>>,
    },
    Text(&'a dyn Array),
    Leaf(&'a dyn Array, ArrayFormatter<'a>),
}

impl<'a> Nested<'a> {
    fn new(array: &'a dyn Array, options: &FormatOptions<'a>) -> Result<Self, ArrowError> {
        Ok(match array.data_type() {
            DataType::List(_) => Self::List {
                array,
                values: Box::new(Self::new(
                    array.as_list::<i32>().values().as_ref(),
                    options,
                )?),
            },
            DataType::LargeList(_) => Self::List {
                array,
                values: Box::new(Self::new(
                    array.as_list::<i64>().values().as_ref(),
                    options,
                )?),
            },
            DataType::FixedSizeList(..) => Self::List {
                array,
                values: Box::new(Self::new(
                    array.as_fixed_size_list().values().as_ref(),
                    options,
                )?),
            },
            DataType::Struct(fields) => Self::Struct {
                array,
                names: fields.iter().map(|f| f.name().clone()).collect(),
                fields: array
                    .as_struct()
                    .columns()
                    .iter()
                    .map(|c| Self::new(c.as_ref(), options))
                    .collect::<Result<_, _>>()?,
            },
            DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => Self::Text(array),
            _ => Self::Leaf(array, formatter(array, options)?),
        })
    }

    /// The element range of list `i`.
    fn range(array: &dyn Array, i: usize) -> std::ops::Range<usize> {
        match array.data_type() {
            DataType::List(_) => {
                let o = array.as_list::<i32>().value_offsets();
                usize::try_from(o[i]).unwrap_or(0)..usize::try_from(o[i + 1]).unwrap_or(0)
            }
            DataType::LargeList(_) => {
                let o = array.as_list::<i64>().value_offsets();
                usize::try_from(o[i]).unwrap_or(0)..usize::try_from(o[i + 1]).unwrap_or(0)
            }
            _ => {
                let n = usize::try_from(array.as_fixed_size_list().value_length()).unwrap_or(0);
                i * n..(i + 1) * n
            }
        }
    }

    fn array(&self) -> &'a dyn Array {
        match self {
            Self::List { array, .. }
            | Self::Struct { array, .. }
            | Self::Text(array)
            | Self::Leaf(array, _) => *array,
        }
    }

    /// Element `i`, NULL included, as a literal.
    fn element(&self, i: usize, f: &mut dyn Write) -> FormatResult {
        if self.array().is_null(i) {
            f.write_str("NULL")?;
            return Ok(());
        }
        match self {
            Self::Text(array) => {
                let text = arrow_cast::display::array_value_to_string(*array, i)?;
                write!(f, "'{}'", text.replace('\'', "''"))?;
            }
            _ => self.write(i, f)?,
        }
        Ok(())
    }
}

impl DisplayIndex for Nested<'_> {
    fn write(&self, idx: usize, f: &mut dyn Write) -> FormatResult {
        match self {
            Self::List { array, values } => {
                f.write_char('[')?;
                for (n, i) in Self::range(*array, idx).enumerate() {
                    if n > 0 {
                        f.write_str(", ")?;
                    }
                    values.element(i, f)?;
                }
                f.write_char(']')?;
            }
            Self::Struct { names, fields, .. } => {
                // Fields named 1, 2, … are a tuple's (ClickHouse): `(1, 'a')`.
                let tuple = names
                    .iter()
                    .enumerate()
                    .all(|(i, name)| *name == (i + 1).to_string());
                f.write_char(if tuple { '(' } else { '{' })?;
                for (n, (name, field)) in names.iter().zip(fields).enumerate() {
                    if n > 0 {
                        f.write_str(", ")?;
                    }
                    if !tuple {
                        write!(f, "'{}': ", name.replace('\'', "''"))?;
                    }
                    field.element(idx, f)?;
                }
                f.write_char(if tuple { ')' } else { '}' })?;
            }
            Self::Text(array) => {
                f.write_str(&arrow_cast::display::array_value_to_string(*array, idx)?)?;
            }
            Self::Leaf(_, inner) => inner.value(idx).write(f)?,
        }
        Ok(())
    }
}

/// A [`Nested`] column, whose NULLs are written as the options say.
struct Top<'a> {
    nested: Nested<'a>,
    null: &'a str,
}

impl DisplayIndex for Top<'_> {
    fn write(&self, idx: usize, f: &mut dyn Write) -> FormatResult {
        if self.nested.array().is_null(idx) {
            f.write_str(self.null)?;
            return Ok(());
        }
        self.nested.write(idx, f)
    }
}

/// `batch` with each column that holds a reserved value turned into text, for writers that
/// format values themselves (CSV, JSON): the column's type is kept otherwise.
///
/// # Errors
/// When a column cannot be formatted.
pub fn with_text(batch: &RecordBatch) -> Result<RecordBatch, ArrowError> {
    let schema = batch.schema();
    let special: Vec<bool> = schema
        .fields()
        .iter()
        .zip(batch.columns())
        .map(|(f, c)| find(f, c.as_ref()).is_some())
        .collect();
    if !special.contains(&true) {
        return Ok(batch.clone());
    }
    let options = FormatOptions::default();
    let mut fields = Vec::with_capacity(special.len());
    let mut columns = Vec::with_capacity(special.len());
    for ((field, column), special) in schema.fields().iter().zip(batch.columns()).zip(special) {
        if special {
            let f = column_formatter(field, column.as_ref(), &options)?;
            let texts: StringArray = (0..column.len())
                .map(|i| (!column.is_null(i)).then(|| f.value(i).to_string()))
                .collect();
            fields.push(
                Field::new(field.name(), DataType::Utf8, true)
                    .with_metadata(field.metadata().clone()),
            );
            columns.push(std::sync::Arc::new(texts) as ArrayRef);
        } else {
            fields.push(field.as_ref().clone());
            columns.push(std::sync::Arc::clone(column));
        }
    }
    RecordBatch::try_new(std::sync::Arc::new(Schema::new(fields)), columns)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{Date32Array, Decimal128Array, Int32Array, TimestampMicrosecondArray};

    use super::*;

    fn shown(array: &dyn Array) -> Vec<String> {
        let options = FormatOptions::default().with_null("NULL");
        let f = formatter(array, &options).unwrap();
        (0..array.len()).map(|i| f.value(i).to_string()).collect()
    }

    #[test]
    fn shows_infinities_and_invalid_dates() {
        let dates = Date32Array::from(vec![
            Some(DATE_INFINITY),
            Some(DATE_NEG_INFINITY),
            Some(0),
            invalid_date(0, 0, 0),
            invalid_date(2024, 2, 31),
            None,
        ]);
        assert_eq!(
            shown(&dates),
            [
                "infinity",
                "-infinity",
                "1970-01-01",
                "0000-00-00",
                "2024-02-31",
                "NULL"
            ]
        );
        let stamps = TimestampMicrosecondArray::from(vec![
            Some(TIMESTAMP_INFINITY),
            Some(TIMESTAMP_NEG_INFINITY),
            Some(1_000_000),
            invalid_timestamp(0, 0, 0, 0),
            invalid_timestamp(9_999, 12, 31, MICROS_PER_DAY - 1),
        ])
        .with_timezone("+00:00");
        assert_eq!(
            shown(&stamps),
            [
                "infinity",
                "-infinity",
                "1970-01-01T00:00:01Z",
                "0000-00-00 00:00:00",
                "9999-12-31 23:59:59.999999"
            ]
        );
        assert!(invalid_date(10_000, 1, 1).is_none());
        assert!(invalid_timestamp(2024, 13, 1, 0).is_none());
    }

    #[test]
    fn reserved_values_are_no_real_dates() {
        // chrono shows none of them: no real value can be mistaken for one.
        for days in [
            DATE_INFINITY,
            DATE_NEG_INFINITY,
            invalid_date(9_999, 12, 31).unwrap(),
        ] {
            assert!(arrow_array::temporal_conversions::date32_to_datetime(days).is_none());
        }
        for micros in [
            TIMESTAMP_NEG_INFINITY,
            invalid_timestamp(0, 0, 0, 0).unwrap(),
            invalid_timestamp(9_999, 12, 31, MICROS_PER_DAY - 1).unwrap(),
        ] {
            assert!(arrow_array::temporal_conversions::timestamp_us_to_datetime(micros).is_none());
        }
    }

    #[test]
    fn shows_bc_dates_as_postgresql_does() {
        // 0044-03-15 BC is year -43 to chrono.
        let ides = chrono_days(-43, 3, 15).unwrap();
        let dates = Date32Array::from(vec![ides, chrono_days(0, 1, 1).unwrap()]);
        assert_eq!(shown(&dates), ["0044-03-15 BC", "0001-01-01 BC"]);
        let noon = TimestampMicrosecondArray::from(vec![
            i64::from(ides) * MICROS_PER_DAY + 43_200_000_000,
        ]);
        assert_eq!(shown(&noon), ["0044-03-15T12:00:00 BC"]);
        assert_eq!(bc_text("2024-01-01"), None);
    }

    /// Days from the epoch to that day of the proleptic Gregorian calendar.
    fn chrono_days(y: i64, m: i64, d: i64) -> Option<i32> {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        i32::try_from(era * 146_097 + doe - 719_468).ok()
    }

    #[test]
    fn shows_lists_and_structs_as_literals() {
        let list = arrow_array::ListArray::from_iter_primitive::<arrow_array::types::Int32Type, _, _>(
            vec![Some(vec![Some(1), None, Some(3)]), None],
        );
        assert_eq!(shown(&list), ["[1, NULL, 3]", "NULL"]);
        let mut texts =
            arrow_array::builder::ListBuilder::new(arrow_array::builder::StringBuilder::new());
        texts.append_value([Some("a"), Some(""), Some("it's"), None]);
        let list = texts.finish();
        assert_eq!(shown(&list), ["['a', '', 'it''s', NULL]"]);
        let st = arrow_array::StructArray::from(vec![
            (
                Arc::new(Field::new("a", DataType::Int32, true)),
                Arc::new(Int32Array::from(vec![1])) as ArrayRef,
            ),
            (
                Arc::new(Field::new("b", DataType::Utf8, true)),
                Arc::new(StringArray::from(vec!["x"])) as ArrayRef,
            ),
        ]);
        assert_eq!(shown(&st), ["{'a': 1, 'b': 'x'}"]);
    }

    fn column_shown(field: &Field, array: &dyn Array) -> Vec<String> {
        let options = FormatOptions::default().with_null("NULL");
        let f = column_formatter(field, array, &options).unwrap();
        (0..array.len()).map(|i| f.value(i).to_string()).collect()
    }

    #[test]
    fn shows_decimal_nan_only_in_a_marked_field() {
        let decimals = Decimal128Array::from(vec![Some(150), Some(DECIMAL_NAN), None])
            .with_precision_and_scale(10, 2)
            .unwrap();
        let numeric = with_decimal_nan(Field::new("n", decimals.data_type().clone(), true));
        assert_eq!(column_shown(&numeric, &decimals), ["1.50", "NaN", "NULL"]);
        assert_eq!(find(&numeric, &decimals), Some((1, "NaN".to_owned())));
        assert_eq!(column_text(&numeric, &decimals, 1).as_deref(), Some("NaN"));
        // Anywhere else i128::MAX is a number: DuckDB's largest HUGEINT.
        let huge = Decimal128Array::from(vec![Some(DECIMAL_NAN), Some(-DECIMAL_NAN)])
            .with_precision_and_scale(38, 0)
            .unwrap();
        let plain = Field::new("h", huge.data_type().clone(), true);
        assert_eq!(
            column_shown(&plain, &huge),
            [
                "170141183460469231731687303715884105727",
                "-170141183460469231731687303715884105727"
            ]
        );
        assert_eq!(find(&plain, &huge), None);
        assert_eq!(shown(&huge)[0], "170141183460469231731687303715884105727");
        let ints = Field::new("i", DataType::Int32, true);
        assert_eq!(find(&ints, &Int32Array::from(vec![i32::MAX])), None);
    }

    #[test]
    fn a_marked_decimal_column_with_nan_turns_into_text() {
        let decimals = Decimal128Array::from(vec![Some(150), Some(DECIMAL_NAN)])
            .with_precision_and_scale(10, 2)
            .unwrap();
        let numeric = with_decimal_nan(Field::new("n", decimals.data_type().clone(), true));
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![numeric])),
            vec![Arc::new(decimals)],
        )
        .unwrap();
        let text = with_text(&batch).unwrap();
        let n = text.column(0).as_string::<i32>();
        assert_eq!((n.value(0), n.value(1)), ("1.50", "NaN"));
    }

    #[test]
    fn turns_columns_with_reserved_values_into_text() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("d", DataType::Date32, true),
            Field::new("n", DataType::Int32, true),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Date32Array::from(vec![Some(DATE_INFINITY), Some(1), None])),
                Arc::new(Int32Array::from(vec![1, 2, 3])),
            ],
        )
        .unwrap();
        let text = with_text(&batch).unwrap();
        assert_eq!(text.schema().field(0).data_type(), &DataType::Utf8);
        assert_eq!(text.schema().field(1).data_type(), &DataType::Int32);
        let d = text.column(0).as_string::<i32>();
        assert_eq!(d.value(0), "infinity");
        assert_eq!(d.value(1), "1970-01-02");
        assert!(d.is_null(2));
        let plain = with_text(&batch.slice(1, 2)).unwrap();
        assert_eq!(plain.schema().field(0).data_type(), &DataType::Date32);
    }
}
