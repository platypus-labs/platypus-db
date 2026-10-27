//! The profile of one result column, for the box under its header: NULLs, distinct values,
//! extremes, and a small histogram (numbers and times) or the most frequent values (text).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::hash::{BuildHasher, RandomState};

use arrow_array::{Array, RecordBatch};
use arrow_cast::display::FormatOptions;
use arrow_row::{OwnedRow, RowConverter, SortField};
use arrow_schema::{ArrowError, DataType, Schema};
use serde::Serialize;

use crate::LocalError;
use crate::values::{self, Kind};

/// Ranges of the histogram of numbers and times.
pub const BINS: u8 = 20;
/// Most frequent text values reported.
pub const TOP: usize = 5;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ColumnProfile {
    /// Rows looked at.
    pub rows: usize,
    pub nulls: usize,
    /// Distinct non-NULL values, as shown.
    pub distinct: usize,
    /// Smallest and largest value as shown: by value for numbers and times, by text otherwise;
    /// `None` for binary, JSON and composite columns and when every value is NULL.
    pub min: Option<String>,
    pub max: Option<String>,
    /// Numbers and times: `BINS` equal ranges, or one when every value is the same.
    pub histogram: Vec<Bin>,
    /// Text: the `TOP` most frequent values, most frequent first.
    pub top: Vec<TopValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bin {
    pub from: String,
    pub to: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopValue {
    pub value: String,
    pub count: usize,
}

/// The profile of `column` over `batches`, values shown with `options`.
pub fn profile(
    schema: &Schema,
    batches: &[RecordBatch],
    column: usize,
    options: &FormatOptions<'_>,
) -> Result<ColumnProfile, LocalError> {
    let field = values::field(schema, column)?;
    let mut p = ColumnProfile::default();
    for batch in batches {
        let array = batch.column(column);
        p.rows += array.len();
        p.nulls += array.null_count();
    }
    match values::kind(field) {
        Kind::Text => {
            text_profile(&mut p, batches, column, options)?;
            if values::is_span(field.data_type()) {
                span_extremes(&mut p, batches, column, options)?;
            }
        }
        kind @ (Kind::Number | Kind::Time) => {
            p.distinct = distinct(batches, column, options)?;
            let time = kind == Kind::Time;
            ranges(&mut p, batches, column, field.data_type(), time, options)?;
        }
        Kind::Opaque => p.distinct = distinct(batches, column, options)?,
    }
    Ok(p)
}

/// Distinct values, counted by the hash of their text: a collision is too rare to matter.
fn distinct(
    batches: &[RecordBatch],
    column: usize,
    options: &FormatOptions<'_>,
) -> Result<usize, ArrowError> {
    let hasher = RandomState::new();
    let mut seen = HashSet::new();
    let mut text = String::new();
    for batch in batches {
        let array = batch.column(column);
        let formatter = platypus_core::special::formatter(array.as_ref(), options)?;
        for row in (0..array.len()).filter(|&r| array.is_valid(r)) {
            text.clear();
            let _ = write!(text, "{}", formatter.value(row));
            seen.insert(hasher.hash_one(&text));
        }
    }
    Ok(seen.len())
}

/// Text: distinct values, the first and the last in text order, the most frequent ones.
fn text_profile(
    p: &mut ColumnProfile,
    batches: &[RecordBatch],
    column: usize,
    options: &FormatOptions<'_>,
) -> Result<(), ArrowError> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for batch in batches {
        for value in values::texts(batch.column(column).as_ref(), options)?
            .into_iter()
            .flatten()
        {
            *counts.entry(value).or_default() += 1;
        }
    }
    p.distinct = counts.len();
    p.min = counts.keys().min().cloned();
    p.max = counts.keys().max().cloned();
    let mut top: Vec<TopValue> = counts
        .into_iter()
        .map(|(value, count)| TopValue { value, count })
        .collect();
    let order =
        |a: &TopValue, b: &TopValue| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value));
    if top.len() > TOP {
        top.select_nth_unstable_by(TOP, order);
        top.truncate(TOP);
    }
    top.sort_unstable_by(order);
    p.top = top;
    Ok(())
}

/// Numbers and times: the extremes as the grid shows them, and the histogram, whose ends are
/// the extremes.
fn ranges(
    p: &mut ColumnProfile,
    batches: &[RecordBatch],
    column: usize,
    data_type: &DataType,
    time: bool,
    options: &FormatOptions<'_>,
) -> Result<(), ArrowError> {
    let Some((lo, hi)) = extremes(batches, column, data_type)? else {
        return Ok(());
    };
    let min = shown(batches, column, lo, options)?;
    let max = shown(batches, column, hi, options)?;
    // The ranges cut the finite values; infinities count in the first or the last one.
    let mut numbers = Vec::new();
    for batch in batches {
        let all = values::numbers(batch.column(column))?;
        numbers.extend(all.iter().flatten().filter(|v| !v.is_nan()));
    }
    let (flo, fhi) = numbers
        .iter()
        .filter(|v| v.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    let mut bins = if flo <= fhi {
        histogram(numbers.iter().copied(), flo, fhi)
    } else {
        vec![(0.0, 0.0, numbers.len())]
    };
    let label = |v: f64| {
        if time {
            values::time_text(data_type, values::whole(v), options)
        } else {
            Ok(values::format_number(v))
        }
    };
    let last = bins.len().saturating_sub(1);
    p.histogram = bins
        .drain(..)
        .enumerate()
        .map(|(i, (from, to, count))| {
            Ok(Bin {
                from: if i == 0 { min.clone() } else { label(from)? },
                to: if i == last { max.clone() } else { label(to)? },
                count,
            })
        })
        .collect::<Result<_, ArrowError>>()?;
    p.min = Some(min);
    p.max = Some(max);
    Ok(())
}

/// Where a value is: its batch and its row.
type Spot = (usize, usize);

/// A value's key and where the value is.
type Keyed<K> = (K, Spot);

/// The first smallest and the first largest of `items`.
fn pick<K: Ord + Copy>(mut items: impl Iterator<Item = Keyed<K>>) -> Option<(Keyed<K>, Keyed<K>)> {
    let first = items.next()?;
    Some(items.fold((first, first), |(lo, hi), item| {
        (
            if item.0 < lo.0 { item } else { lo },
            if item.0 > hi.0 { item } else { hi },
        )
    }))
}

/// The smallest and the largest value of a number or time column, compared exactly in their
/// own type (integers past 2^53, wide decimals); infinities count, NaN does not.
fn extremes(
    batches: &[RecordBatch],
    column: usize,
    data_type: &DataType,
) -> Result<Option<(Spot, Spot)>, ArrowError> {
    let converter = RowConverter::new(vec![SortField::new(data_type.clone())])?;
    let mut low: Option<(OwnedRow, Spot)> = None;
    let mut high: Option<(OwnedRow, Spot)> = None;
    for (b, batch) in batches.iter().enumerate() {
        let array = batch.column(column);
        let rows = converter.convert_columns(std::slice::from_ref(array))?;
        let numbers = values::numbers(array)?;
        let valid = (0..array.len())
            .filter(|&r| array.is_valid(r) && !(numbers.is_valid(r) && numbers.value(r).is_nan()))
            .map(|r| (rows.row(r), (b, r)));
        let Some(((lo, lo_at), (hi, hi_at))) = pick(valid) else {
            continue;
        };
        if low.as_ref().is_none_or(|(l, _)| lo < l.row()) {
            low = Some((lo.owned(), lo_at));
        }
        if high.as_ref().is_none_or(|(h, _)| hi > h.row()) {
            high = Some((hi.owned(), hi_at));
        }
    }
    Ok(low.zip(high).map(|((_, lo), (_, hi))| (lo, hi)))
}

/// Intervals and durations: the first and the last by length, not by text.
fn span_extremes(
    p: &mut ColumnProfile,
    batches: &[RecordBatch],
    column: usize,
    options: &FormatOptions<'_>,
) -> Result<(), ArrowError> {
    let mut low: Option<Keyed<i128>> = None;
    let mut high: Option<Keyed<i128>> = None;
    for (b, batch) in batches.iter().enumerate() {
        let spans = values::spans(batch.column(column))?;
        let found = spans
            .iter()
            .enumerate()
            .filter_map(|(r, s)| s.map(|s| (s, (b, r))));
        let Some((lo, hi)) = pick(found) else {
            continue;
        };
        if low.is_none_or(|l| lo.0 < l.0) {
            low = Some(lo);
        }
        if high.is_none_or(|h| hi.0 > h.0) {
            high = Some(hi);
        }
    }
    if let Some(((_, lo), (_, hi))) = low.zip(high) {
        p.min = Some(shown(batches, column, lo, options)?);
        p.max = Some(shown(batches, column, hi, options)?);
    }
    Ok(())
}

/// The value at `spot`, as the grid shows it.
fn shown(
    batches: &[RecordBatch],
    column: usize,
    (batch, row): Spot,
    options: &FormatOptions<'_>,
) -> Result<String, ArrowError> {
    let array = batches[batch].column(column);
    Ok(platypus_core::special::formatter(array.as_ref(), options)?
        .value(row)
        .to_string())
}

/// `BINS` equal ranges from `lo` to `hi` (the last one closed) and how many of `numbers` fall
/// in each; a single range when they are all `lo`.
pub fn histogram(numbers: impl Iterator<Item = f64>, lo: f64, hi: f64) -> Vec<(f64, f64, usize)> {
    if hi <= lo {
        return vec![(lo, hi, numbers.count())];
    }
    let bins = usize::from(BINS);
    // Divided first: `hi - lo` overflows to infinity for extremes of opposite sign.
    let width = hi / f64::from(BINS) - lo / f64::from(BINS);
    let mut counts = vec![0; bins];
    for v in numbers {
        counts[values::slot(v, lo, width, bins)] += 1;
    }
    // The start of range `i`; weighted between the ends where `lo + width * i` overflows.
    let bound = |i: u8| {
        let at = lo + width * f64::from(i);
        if at.is_finite() {
            return at;
        }
        let t = f64::from(i) / f64::from(BINS);
        lo * (1.0 - t) + hi * t
    };
    (0..BINS)
        .zip(counts)
        .map(|(i, count)| (bound(i), bound(i + 1), count))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use arrow_array::types::IntervalMonthDayNanoType;
    use arrow_array::{
        ArrayRef, BinaryArray, Date32Array, Decimal128Array, DurationSecondArray, Float64Array,
        Int32Array, Int64Array, IntervalMonthDayNanoArray, StringArray, UInt64Array,
    };
    use arrow_schema::Field;
    use platypus_core::META_DB_TYPE;

    use super::*;

    fn of(field: Field, array: ArrayRef) -> ColumnProfile {
        let schema = Schema::new(vec![field]);
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![array]).unwrap();
        profile(&schema, &[batch], 0, &FormatOptions::default()).unwrap()
    }

    #[test]
    fn numbers() {
        let values = Int32Array::from(vec![Some(1), None, Some(10), Some(10), Some(4)]);
        let p = of(Field::new("n", DataType::Int32, true), Arc::new(values));
        assert_eq!((p.rows, p.nulls, p.distinct), (5, 1, 3));
        assert_eq!(
            (p.min.as_deref(), p.max.as_deref()),
            (Some("1"), Some("10"))
        );
        assert_eq!(p.histogram.len(), 20);
        assert_eq!(p.histogram.iter().map(|b| b.count).sum::<usize>(), 4);
        assert_eq!(p.histogram[0].from, "1");
        assert_eq!(
            (p.histogram[19].to.as_str(), p.histogram[19].count),
            ("10", 2)
        );
        assert_eq!(p.top, Vec::<TopValue>::new());
    }

    #[test]
    fn dates() {
        // 2024-01-01 and twice 2024-01-31.
        let days = Date32Array::from(vec![19_723, 19_753, 19_753]);
        let p = of(Field::new("d", DataType::Date32, true), Arc::new(days));
        assert_eq!(
            (p.min.as_deref(), p.max.as_deref()),
            (Some("2024-01-01"), Some("2024-01-31"))
        );
        assert_eq!(p.distinct, 2);
        assert_eq!(
            p.histogram.first().map(|b| b.from.as_str()),
            Some("2024-01-01")
        );
        assert_eq!(
            p.histogram.last().map(|b| (b.to.as_str(), b.count)),
            Some(("2024-01-31", 2))
        );
    }

    #[test]
    fn text() {
        let values = StringArray::from(vec![Some("b"), Some("a"), None, Some("b"), Some("c")]);
        let p = of(Field::new("s", DataType::Utf8, true), Arc::new(values));
        assert_eq!((p.nulls, p.distinct), (1, 3));
        assert_eq!((p.min.as_deref(), p.max.as_deref()), (Some("a"), Some("c")));
        assert_eq!(
            p.top[0],
            TopValue {
                value: "b".to_owned(),
                count: 2
            }
        );
        let order: Vec<&str> = p.top.iter().map(|t| t.value.as_str()).collect();
        assert_eq!(order, ["b", "a", "c"]);
        assert_eq!(p.histogram, Vec::<Bin>::new());
    }

    #[test]
    fn only_nulls_and_no_rows() {
        let nulls = StringArray::from(vec![None::<&str>, None]);
        let p = of(Field::new("s", DataType::Utf8, true), Arc::new(nulls));
        assert_eq!((p.rows, p.nulls, p.distinct, p.min), (2, 2, 0, None));
        let schema = Schema::new(vec![Field::new("n", DataType::Int32, true)]);
        let empty = profile(&schema, &[], 0, &FormatOptions::default()).unwrap();
        assert_eq!(empty, ColumnProfile::default());
    }

    #[test]
    fn binary_and_json_are_only_counted() {
        let bytes = BinaryArray::from(vec![Some(b"x".as_ref()), Some(b"x".as_ref()), None]);
        let p = of(Field::new("b", DataType::Binary, true), Arc::new(bytes));
        assert_eq!((p.distinct, p.nulls, p.min, p.top.len()), (1, 1, None, 0));
        let json = Field::new("j", DataType::Utf8, true).with_metadata(HashMap::from([(
            META_DB_TYPE.to_owned(),
            "jsonb".to_owned(),
        )]));
        let p = of(json, Arc::new(StringArray::from(vec!["{}", "[]"])));
        assert_eq!((p.distinct, p.min, p.top.len()), (2, None, 0));
    }

    #[test]
    fn equal_numbers_make_one_range() {
        let p = of(
            Field::new("n", DataType::Int32, false),
            Arc::new(Int32Array::from(vec![7, 7])),
        );
        let one = Bin {
            from: "7".to_owned(),
            to: "7".to_owned(),
            count: 2,
        };
        assert_eq!(p.histogram, [one]);
    }

    #[test]
    fn extreme_ranges_have_finite_bounds() {
        let bins = histogram([-f64::MAX, 0.0, f64::MAX].into_iter(), -f64::MAX, f64::MAX);
        assert_eq!(bins.len(), usize::from(BINS));
        assert!(
            bins.iter()
                .all(|(from, to, _)| from.is_finite() && to.is_finite())
        );
        assert_eq!(bins.iter().map(|b| b.2).sum::<usize>(), 3);
        assert_eq!(bins.last().map(|b| b.2), Some(1));
    }

    fn extremes_of(array: ArrayRef) -> (Option<String>, Option<String>, String, String) {
        let field = Field::new("v", array.data_type().clone(), true);
        let p = of(field, array);
        let from = p
            .histogram
            .first()
            .map(|b| b.from.clone())
            .unwrap_or_default();
        let to = p.histogram.last().map(|b| b.to.clone()).unwrap_or_default();
        (p.min, p.max, from, to)
    }

    fn pair(min: &str, max: &str) -> (Option<String>, Option<String>, String, String) {
        (
            Some(min.to_owned()),
            Some(max.to_owned()),
            min.to_owned(),
            max.to_owned(),
        )
    }

    #[test]
    fn big_integers_are_compared_exactly() {
        let ints = Int64Array::from(vec![9_007_199_254_740_993, 9_007_199_254_740_992]);
        assert_eq!(
            extremes_of(Arc::new(ints)),
            pair("9007199254740992", "9007199254740993")
        );
        let unsigned = UInt64Array::from(vec![u64::MAX, u64::MAX - 1, 0]);
        assert_eq!(
            extremes_of(Arc::new(unsigned)),
            pair("0", "18446744073709551615")
        );
        let ends = Int64Array::from(vec![0, i64::MAX]);
        assert_eq!(
            extremes_of(Arc::new(ends)),
            pair("0", "9223372036854775807")
        );
        // `i128::MAX` is the reserved numeric NaN, left out of the extremes as NaN is.
        let huge = Decimal128Array::from(vec![i128::MAX - 1, i128::MAX - 2, i128::MAX])
            .with_precision_and_scale(38, 0)
            .unwrap();
        let (min, max, ..) = extremes_of(Arc::new(huge));
        assert_eq!(min, Some((i128::MAX - 2).to_string()));
        assert_eq!(max, Some((i128::MAX - 1).to_string()));
    }

    #[test]
    fn infinities_are_extremes_and_nan_is_not() {
        let floats = Float64Array::from(vec![1.5, f64::INFINITY, f64::NEG_INFINITY, 2.5, f64::NAN]);
        let field = Field::new("v", DataType::Float64, true);
        let p = of(field, Arc::new(floats));
        assert_eq!(
            (p.min.as_deref(), p.max.as_deref()),
            (Some("-inf"), Some("inf"))
        );
        assert_eq!(p.histogram.iter().map(|b| b.count).sum::<usize>(), 4);
        assert_eq!(p.histogram[0].count, 2, "-inf with 1.5");
        let only = Float64Array::from(vec![f64::INFINITY, f64::INFINITY]);
        let p = of(Field::new("v", DataType::Float64, true), Arc::new(only));
        assert_eq!(p.max.as_deref(), Some("inf"));
        assert_eq!(p.histogram.len(), 1);
    }

    #[test]
    fn intervals_by_length() {
        let spans = IntervalMonthDayNanoArray::from(vec![
            Some(IntervalMonthDayNanoType::make_value(0, 1, 0)),
            Some(IntervalMonthDayNanoType::make_value(
                0,
                0,
                7_200_000_000_000,
            )),
            None,
            Some(IntervalMonthDayNanoType::make_value(1, 0, 0)),
            Some(IntervalMonthDayNanoType::make_value(0, 40, 0)),
        ]);
        let field = Field::new("iv", spans.data_type().clone(), true);
        let p = of(field, Arc::new(spans));
        let shown = |v: &IntervalMonthDayNanoArray, i: usize| {
            values::texts(v, &FormatOptions::default()).unwrap()[i].clone()
        };
        let all = IntervalMonthDayNanoArray::from(vec![
            IntervalMonthDayNanoType::make_value(0, 0, 7_200_000_000_000),
            IntervalMonthDayNanoType::make_value(0, 40, 0),
        ]);
        assert_eq!(p.min, shown(&all, 0), "2 hours");
        assert_eq!(p.max, shown(&all, 1), "40 days is longer than a month");
        let seconds = DurationSecondArray::from(vec![60, -5, 3_600]);
        let field = Field::new("d", seconds.data_type().clone(), true);
        let p = of(field, Arc::new(seconds.clone()));
        let texts = values::texts(&seconds, &FormatOptions::default()).unwrap();
        assert_eq!((p.min, p.max), (texts[1].clone(), texts[2].clone()));
    }

    #[test]
    fn a_missing_column_is_an_error() {
        let schema = Schema::new(vec![Field::new("n", DataType::Int32, true)]);
        let result = profile(&schema, &[], 3, &FormatOptions::default());
        assert!(matches!(result, Err(LocalError::NoColumn(3))));
    }
}
