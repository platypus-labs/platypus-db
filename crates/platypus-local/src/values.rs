//! Reading Arrow columns the way the analysis needs them: as numbers, as milliseconds since the
//! epoch, or as the text the grid shows.

use std::sync::Arc;

use arrow_array::cast::AsArray;
use arrow_array::types::{Float64Type, Int64Type};
use arrow_array::{Array, ArrayRef, Float64Array, Int64Array, TimestampMillisecondArray};
use arrow_cast::display::FormatOptions;
use arrow_schema::{ArrowError, DataType, Field, Schema, TimeUnit};
use platypus_core::META_DB_TYPE;

use crate::LocalError;

/// How a column is analysed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Integers, floats and decimals.
    Number,
    /// Dates and timestamps, read as milliseconds since the epoch.
    Time,
    /// Text, booleans, times of day, durations: compared as the text shown.
    Text,
    /// Binary, JSON and composite values: only counted.
    Opaque,
}

/// How `field` is analysed; JSON is told by the database type the driver reported.
#[must_use]
pub fn kind(field: &Field) -> Kind {
    let json = field
        .metadata()
        .get(META_DB_TYPE)
        .is_some_and(|t| t.to_ascii_lowercase().contains("json"));
    match field.data_type() {
        _ if json => Kind::Opaque,
        t if t.is_numeric() => Kind::Number,
        DataType::Date32 | DataType::Date64 | DataType::Timestamp(_, _) => Kind::Time,
        DataType::Utf8
        | DataType::LargeUtf8
        | DataType::Utf8View
        | DataType::Boolean
        | DataType::Time32(_)
        | DataType::Time64(_)
        | DataType::Duration(_)
        | DataType::Interval(_) => Kind::Text,
        DataType::Dictionary(_, value)
            if matches!(value.as_ref(), DataType::Utf8 | DataType::LargeUtf8) =>
        {
            Kind::Text
        }
        _ => Kind::Opaque,
    }
}

/// The field of `column`, or the error that the result has no such column.
pub fn field(schema: &Schema, column: usize) -> Result<&Field, LocalError> {
    schema
        .fields()
        .get(column)
        .map(AsRef::as_ref)
        .ok_or(LocalError::NoColumn(column))
}

/// The values as `f64`: numbers as they are, dates and timestamps as milliseconds since the
/// epoch, text that reads as a number; anything else is NULL.
pub fn numbers(array: &ArrayRef) -> Result<Float64Array, ArrowError> {
    let source: ArrayRef = match array.data_type() {
        DataType::Date32 | DataType::Date64 | DataType::Timestamp(_, _) => Arc::new(millis(array)?),
        _ => Arc::clone(array),
    };
    if !arrow_cast::can_cast_types(source.data_type(), &DataType::Float64) {
        return Ok(Float64Array::new_null(array.len()));
    }
    // The default cast is safe: what does not convert becomes NULL.
    let cast = arrow_cast::cast(&source, &DataType::Float64)?;
    Ok(cast.as_primitive::<Float64Type>().clone())
}

/// Dates and timestamps as milliseconds since the epoch.
pub fn millis(array: &ArrayRef) -> Result<Int64Array, ArrowError> {
    let stamps = arrow_cast::cast(array, &DataType::Timestamp(TimeUnit::Millisecond, None))?;
    let ints = arrow_cast::cast(&stamps, &DataType::Int64)?;
    Ok(ints.as_primitive::<Int64Type>().clone())
}

/// The time `ms` as a value of `data_type` shows it: a date for dates, else a timestamp.
pub fn time_text(
    data_type: &DataType,
    ms: i64,
    options: &FormatOptions<'_>,
) -> Result<String, ArrowError> {
    let stamp: ArrayRef = Arc::new(TimestampMillisecondArray::from(vec![ms]));
    let value = match data_type {
        DataType::Date32 | DataType::Date64 => arrow_cast::cast(&stamp, &DataType::Date32)?,
        _ => stamp,
    };
    Ok(platypus_core::special::formatter(value.as_ref(), options)?
        .value(0)
        .to_string())
}

/// The text of every value as the grid shows it; `None` for NULL.
pub fn texts(
    array: &dyn Array,
    options: &FormatOptions<'_>,
) -> Result<Vec<Option<String>>, ArrowError> {
    let formatter = platypus_core::special::formatter(array, options)?;
    Ok((0..array.len())
        .map(|row| {
            array
                .is_valid(row)
                .then(|| formatter.value(row).to_string())
        })
        .collect())
}

/// A number for labels: whole numbers without decimals, tiny ones in scientific notation,
/// others with up to six decimals.
#[must_use]
pub fn format_number(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    if value.abs() > 0.0 && value.abs() < 1e-4 {
        return format!("{value:e}");
    }
    let mut text = format!("{value:.6}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

/// Which of `count` ranges of `width` starting at `lo` holds `value`; values past the ends go
/// to the first or the last range.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a non-negative position, clamped below `count`"
)]
#[must_use]
pub fn slot(value: f64, lo: f64, width: f64, count: usize) -> usize {
    (((value - lo) / width).floor().max(0.0) as usize).min(count.saturating_sub(1))
}

/// `value` as a whole number, e.g. milliseconds back from `f64`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "times and positions are far within i64"
)]
#[must_use]
pub fn whole(value: f64) -> i64 {
    value.round() as i64
}

/// A count as `f64`, for averages and positions.
#[expect(
    clippy::cast_precision_loss,
    reason = "counts of loaded rows are far below 2^52"
)]
#[must_use]
pub fn float(n: usize) -> f64 {
    n as f64
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use arrow_array::{Date32Array, Int32Array, StringArray, TimestampSecondArray};

    use super::*;

    #[test]
    fn kinds_of_columns() {
        assert_eq!(kind(&Field::new("a", DataType::Int64, true)), Kind::Number);
        assert_eq!(
            kind(&Field::new("a", DataType::Decimal128(10, 2), true)),
            Kind::Number
        );
        assert_eq!(kind(&Field::new("a", DataType::Date32, true)), Kind::Time);
        let stamp = DataType::Timestamp(TimeUnit::Microsecond, None);
        assert_eq!(kind(&Field::new("a", stamp, true)), Kind::Time);
        assert_eq!(kind(&Field::new("a", DataType::Utf8, true)), Kind::Text);
        assert_eq!(kind(&Field::new("a", DataType::Boolean, true)), Kind::Text);
        assert_eq!(kind(&Field::new("a", DataType::Binary, true)), Kind::Opaque);
        let json = Field::new("a", DataType::Utf8, true).with_metadata(HashMap::from([(
            META_DB_TYPE.to_owned(),
            "JSONB".to_owned(),
        )]));
        assert_eq!(kind(&json), Kind::Opaque);
    }

    #[test]
    fn numbers_from_numbers_and_text() {
        let ints: ArrayRef = Arc::new(Int32Array::from(vec![Some(2), None]));
        assert_eq!(
            numbers(&ints).unwrap().iter().collect::<Vec<_>>(),
            [Some(2.0), None]
        );
        let text: ArrayRef = Arc::new(StringArray::from(vec!["2.5", "x"]));
        assert_eq!(
            numbers(&text).unwrap().iter().collect::<Vec<_>>(),
            [Some(2.5), None]
        );
    }

    #[test]
    fn times_as_milliseconds_and_back() {
        let day: ArrayRef = Arc::new(Date32Array::from(vec![1]));
        assert_eq!(millis(&day).unwrap().value(0), 86_400_000);
        let stamp: ArrayRef = Arc::new(TimestampSecondArray::from(vec![60]));
        assert_eq!(millis(&stamp).unwrap().value(0), 60_000);
        let options = FormatOptions::default();
        assert_eq!(
            time_text(&DataType::Date32, 86_400_000, &options).unwrap(),
            "1970-01-02"
        );
        let seconds = DataType::Timestamp(TimeUnit::Second, None);
        assert_eq!(
            time_text(&seconds, 60_000, &options).unwrap(),
            "1970-01-01T00:01:00"
        );
    }

    #[test]
    fn texts_as_shown() {
        let ints: ArrayRef = Arc::new(Int32Array::from(vec![Some(7), None]));
        let shown = texts(ints.as_ref(), &FormatOptions::default()).unwrap();
        assert_eq!(shown, [Some("7".to_owned()), None]);
    }

    #[test]
    fn numbers_for_labels() {
        assert_eq!(format_number(12.0), "12");
        assert_eq!(format_number(-0.5), "-0.5");
        assert_eq!(format_number(1.234_567_89), "1.234568");
        assert_eq!(format_number(0.000_012), "1.2e-5");
    }

    #[test]
    fn slots_clamp_to_the_ends() {
        assert_eq!(slot(0.0, 0.0, 1.0, 4), 0);
        assert_eq!(slot(2.5, 0.0, 1.0, 4), 2);
        assert_eq!(slot(4.0, 0.0, 1.0, 4), 3);
        assert_eq!(slot(-1.0, 0.0, 1.0, 4), 0);
    }
}
