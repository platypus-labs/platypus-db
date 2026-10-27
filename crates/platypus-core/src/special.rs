//! Values that a typed Arrow column cannot hold, but some databases store: `±infinity` dates
//! and timestamps (PostgreSQL), dates outside the calendar such as MySQL's `0000-00-00`, and
//! `NaN` in a PostgreSQL `numeric(p,s)`.
//!
//! Drivers keep the column's type and store these values as reserved numbers that no real
//! value uses: outside the dates chrono can show, or above the largest decimal of 38 digits.
//! [`formatter`] shows them as the text the database prints and reads back (`infinity`,
//! `0000-00-00`, `NaN`); every place that turns values into text goes through it. The
//! infinities are DuckDB's own, so DuckDB and Parquet readers see them as infinities too.

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
/// A `Decimal128` `NaN`: above the largest value of 38 digits.
pub const DECIMAL_NAN: i128 = i128::MAX;

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

/// The text of value `i` of `array` when it is one of the reserved values; `None` for any
/// other value, NULL included.
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
        DataType::Decimal128(..) => (array.as_primitive::<Decimal128Type>().value(i)
            == DECIMAL_NAN)
            .then(|| "NaN".to_owned()),
        _ => None,
    }
}

/// Whether arrays of this type can hold reserved values.
fn may_hold(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::Date32 | DataType::Timestamp(..) | DataType::Decimal128(..)
    )
}

/// The first reserved value of `array`, with its row.
#[must_use]
pub fn find(array: &dyn Array) -> Option<(usize, String)> {
    if !may_hold(array.data_type()) {
        return None;
    }
    (0..array.len()).find_map(|i| text(array, i).map(|t| (i, t)))
}

struct Special<'a> {
    array: &'a dyn Array,
    inner: ArrayFormatter<'a>,
}

impl DisplayIndex for Special<'_> {
    fn write(&self, idx: usize, f: &mut dyn Write) -> FormatResult {
        match text(self.array, idx) {
            Some(text) => f.write_str(&text)?,
            None => self.inner.value(idx).write(f)?,
        }
        Ok(())
    }
}

/// [`ArrayFormatter::try_new`], with the reserved values shown as their text.
///
/// # Errors
/// When Arrow cannot format arrays of this type.
pub fn formatter<'a>(
    array: &'a dyn Array,
    options: &FormatOptions<'a>,
) -> Result<ArrayFormatter<'a>, ArrowError> {
    let inner = ArrayFormatter::try_new(array, options)?;
    if !may_hold(array.data_type()) {
        return Ok(inner);
    }
    Ok(ArrayFormatter::new(
        Box::new(Special { array, inner }),
        options.safe(),
    ))
}

/// `batch` with each column that holds a reserved value turned into text, for writers that
/// format values themselves (CSV, JSON): the column's type is kept otherwise.
///
/// # Errors
/// When a column cannot be formatted.
pub fn with_text(batch: &RecordBatch) -> Result<RecordBatch, ArrowError> {
    let special: Vec<bool> = batch
        .columns()
        .iter()
        .map(|c| find(c.as_ref()).is_some())
        .collect();
    if !special.contains(&true) {
        return Ok(batch.clone());
    }
    let options = FormatOptions::default();
    let schema = batch.schema();
    let mut fields = Vec::with_capacity(special.len());
    let mut columns = Vec::with_capacity(special.len());
    for ((field, column), special) in schema.fields().iter().zip(batch.columns()).zip(special) {
        if special {
            let f = formatter(column.as_ref(), &options)?;
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
    fn shows_decimal_nan_and_finds_it() {
        let decimals = Decimal128Array::from(vec![Some(150), Some(DECIMAL_NAN), None])
            .with_precision_and_scale(10, 2)
            .unwrap();
        assert_eq!(shown(&decimals), ["1.50", "NaN", "NULL"]);
        assert_eq!(find(&decimals), Some((1, "NaN".to_owned())));
        assert_eq!(find(&Int32Array::from(vec![i32::MAX])), None);
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
