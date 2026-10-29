//! The profile of a whole table: for each column what the box under a result column shows
//! ([`stats::profile`]) plus what only a full read can tell: the spread of numbers and their
//! outliers, the length of text, and the shapes its values take (emails, dates, numbers stored
//! as text...) with a few values that break the shape the column mostly has.

use std::sync::Arc;

use arrow_array::RecordBatch;
use arrow_array::cast::AsArray;
use arrow_cast::display::FormatOptions;
use arrow_schema::{DataType, Field, Schema};
use platypus_core::META_DB_TYPE;
use serde::Serialize;

use crate::LocalError;
use crate::patterns::{PatternCounter, PatternReport};
use crate::stats::{self, ColumnProfile};
use crate::values::{self, Kind};

/// Outliers shown as examples, the farthest from the fences first.
pub const OUTLIER_EXAMPLES: usize = 5;
/// Fewer numbers than this have no meaningful quartiles.
const OUTLIER_MIN_VALUES: usize = 4;
/// The IQR rule's multiplier: values past `Q1 - 1.5 IQR` or `Q3 + 1.5 IQR` are outliers.
const FENCE: f64 = 1.5;

/// How the app treats a column, in the words the UI uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    Number,
    Time,
    Text,
    /// Binary, JSON, composite: only counted.
    Other,
}

/// Spread of the numbers of a column.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NumberSummary {
    pub mean: f64,
    /// Sample standard deviation; `None` for a single value.
    pub stddev: Option<f64>,
    pub q1: f64,
    pub median: f64,
    pub q3: f64,
}

/// Length in characters of the values of a text column.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LengthSummary {
    pub min: usize,
    pub avg: f64,
    pub max: usize,
}

/// Values of a numeric column past the IQR fences.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outliers {
    pub count: usize,
    /// Of the numbers in the column, from 0 to 1.
    pub share: f64,
    pub low_fence: f64,
    pub high_fence: f64,
    /// The farthest from the fences, as numbers are shown in labels.
    pub examples: Vec<String>,
}

/// Everything known about one column.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ColumnReport {
    pub name: String,
    /// The Arrow type, e.g. `Utf8`.
    pub data_type: String,
    /// The type the database reported, e.g. `VARCHAR(40)`, when the driver told it.
    pub db_type: Option<String>,
    pub kind: ColumnKind,
    #[serde(flatten)]
    pub profile: ColumnProfile,
    /// Text columns: values that are the empty string (not NULL).
    pub empty: usize,
    pub numbers: Option<NumberSummary>,
    pub lengths: Option<LengthSummary>,
    pub patterns: Option<PatternReport>,
    pub outliers: Option<Outliers>,
}

/// The columns of a table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TableProfile {
    pub rows: usize,
    pub columns: Vec<ColumnReport>,
}

/// Profiles every column of `batches`, values shown with `options`.
pub fn profile_table(
    schema: &Schema,
    batches: &[RecordBatch],
    options: &FormatOptions<'_>,
) -> Result<TableProfile, LocalError> {
    let columns = (0..schema.fields().len())
        .map(|c| column_report(schema, batches, c, options))
        .collect::<Result<_, _>>()?;
    Ok(TableProfile {
        rows: batches.iter().map(RecordBatch::num_rows).sum(),
        columns,
    })
}

/// The report of column `column`.
fn column_report(
    schema: &Schema,
    batches: &[RecordBatch],
    column: usize,
    options: &FormatOptions<'_>,
) -> Result<ColumnReport, LocalError> {
    let field = values::field(schema, column)?;
    let kind = values::kind(field);
    let mut report = ColumnReport {
        name: field.name().clone(),
        data_type: field.data_type().to_string(),
        db_type: field.metadata().get(META_DB_TYPE).cloned(),
        kind: match kind {
            Kind::Number => ColumnKind::Number,
            Kind::Time => ColumnKind::Time,
            Kind::Text => ColumnKind::Text,
            Kind::Opaque => ColumnKind::Other,
        },
        profile: stats::profile(schema, batches, column, options)?,
        empty: 0,
        numbers: None,
        lengths: None,
        patterns: None,
        outliers: None,
    };
    if kind == Kind::Number {
        let mut numbers = finite_numbers(batches, column)?;
        report.numbers = summary(&mut numbers);
        report.outliers = report.numbers.as_ref().and_then(|s| outliers(&numbers, s));
    } else if kind == Kind::Text && is_string(field) {
        text_details(&mut report, batches, column)?;
    }
    Ok(report)
}

/// Whether `field` holds text proper, not booleans, times of day or durations shown as text.
fn is_string(field: &Field) -> bool {
    match field.data_type() {
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => true,
        DataType::Dictionary(_, value) => {
            matches!(value.as_ref(), DataType::Utf8 | DataType::LargeUtf8)
        }
        _ => false,
    }
}

/// The finite numbers of a column, sorted: NULL, `NaN` and infinities leave the spread out.
fn finite_numbers(batches: &[RecordBatch], column: usize) -> Result<Vec<f64>, LocalError> {
    let mut numbers = Vec::new();
    for batch in batches {
        numbers.extend(
            values::column_numbers(batch, column)?
                .iter()
                .flatten()
                .filter(|v| v.is_finite()),
        );
    }
    numbers.sort_unstable_by(f64::total_cmp);
    Ok(numbers)
}

/// Mean, deviation and quartiles of `sorted` (which it sorts if it is not); `None` when empty.
#[must_use]
pub fn summary(sorted: &mut [f64]) -> Option<NumberSummary> {
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_unstable_by(f64::total_cmp);
    #[expect(clippy::cast_precision_loss, reason = "a count of rows fits a double")]
    let n = sorted.len() as f64;
    let mean = sorted.iter().sum::<f64>() / n;
    let stddev = (sorted.len() > 1).then(|| {
        let squares: f64 = sorted.iter().map(|v| (v - mean).powi(2)).sum();
        (squares / (n - 1.0)).sqrt()
    });
    Some(NumberSummary {
        mean,
        stddev,
        q1: quantile(sorted, 0.25),
        median: quantile(sorted, 0.5),
        q3: quantile(sorted, 0.75),
    })
}

/// The `q` quantile of `sorted` (not empty), interpolating between neighbours.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a position among the rows"
)]
fn quantile(sorted: &[f64], q: f64) -> f64 {
    let at = q * (sorted.len() - 1) as f64;
    let (below, above) = (at.floor() as usize, at.ceil() as usize);
    sorted[below] + (sorted[above] - sorted[below]) * (at - at.floor())
}

/// The numbers past the IQR fences. None for a handful of numbers, and none when the middle
/// half is a single value: the fences would be that value and anything else would be flagged.
#[must_use]
pub fn outliers(sorted: &[f64], s: &NumberSummary) -> Option<Outliers> {
    let iqr = s.q3 - s.q1;
    if sorted.len() < OUTLIER_MIN_VALUES || iqr <= 0.0 {
        return None;
    }
    let (low_fence, high_fence) = (s.q1 - FENCE * iqr, s.q3 + FENCE * iqr);
    let outside = |v: &&f64| **v < low_fence || **v > high_fence;
    let mut found: Vec<f64> = sorted.iter().filter(outside).copied().collect();
    let count = found.len();
    if count == 0 {
        return None;
    }
    let distance = |v: f64| (v - s.median).abs();
    found.sort_by(|a, b| distance(*b).total_cmp(&distance(*a)));
    found.dedup();
    #[expect(clippy::cast_precision_loss, reason = "a share needs no exact count")]
    let share = count as f64 / sorted.len() as f64;
    Some(Outliers {
        count,
        share,
        low_fence,
        high_fence,
        examples: found
            .into_iter()
            .take(OUTLIER_EXAMPLES)
            .map(values::format_number)
            .collect(),
    })
}

/// Text: empty strings, lengths and the shapes of the values, read in one pass over the
/// Arrow arrays so that no value is copied but the few the patterns keep.
fn text_details(
    report: &mut ColumnReport,
    batches: &[RecordBatch],
    column: usize,
) -> Result<(), LocalError> {
    let mut lengths = Lengths::default();
    let mut counter = PatternCounter::default();
    for batch in batches {
        let array = batch.column(column);
        let plain = match array.data_type() {
            DataType::Dictionary(_, value) => arrow_cast::cast(array, value)?,
            _ => Arc::clone(array),
        };
        let mut see = |value: &str| {
            lengths.add(value);
            counter.add(value);
        };
        match plain.data_type() {
            DataType::Utf8 => plain.as_string::<i32>().iter().flatten().for_each(&mut see),
            DataType::LargeUtf8 => plain.as_string::<i64>().iter().flatten().for_each(&mut see),
            _ => plain.as_string_view().iter().flatten().for_each(&mut see),
        }
    }
    report.empty = lengths.empty;
    report.lengths = lengths.summary();
    report.patterns = Some(counter.finish());
    Ok(())
}

/// Running shortest, longest and total length of the texts seen, in characters.
#[derive(Default)]
struct Lengths {
    count: usize,
    empty: usize,
    min: usize,
    max: usize,
    sum: usize,
}

impl Lengths {
    fn add(&mut self, text: &str) {
        let size = text.chars().count();
        self.min = if self.count == 0 {
            size
        } else {
            self.min.min(size)
        };
        self.max = self.max.max(size);
        self.sum += size;
        self.count += 1;
        self.empty += usize::from(size == 0);
    }

    /// Shortest, average and longest; `None` when there are no texts.
    fn summary(&self) -> Option<LengthSummary> {
        if self.count == 0 {
            return None;
        }
        #[expect(clippy::cast_precision_loss, reason = "an average needs no exact sum")]
        let avg = self.sum as f64 / self.count as f64;
        Some(LengthSummary {
            min: self.min,
            avg,
            max: self.max,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, BinaryArray, Date32Array, Float64Array, Int64Array, StringArray};

    use super::*;
    use crate::patterns::Pattern;

    fn table(columns: Vec<(&str, ArrayRef)>) -> (Schema, Vec<RecordBatch>) {
        let fields: Vec<Field> = columns
            .iter()
            .map(|(n, a)| Field::new(*n, a.data_type().clone(), true))
            .collect();
        let schema = Schema::new(fields);
        let arrays = columns.into_iter().map(|(_, a)| a).collect();
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), arrays).unwrap();
        (schema, vec![batch])
    }

    fn run(columns: Vec<(&str, ArrayRef)>) -> TableProfile {
        let (schema, batches) = table(columns);
        profile_table(&schema, &batches, &FormatOptions::default()).unwrap()
    }

    #[test]
    fn quartiles_and_spread() {
        let mut v = vec![4.0, 1.0, 3.0, 2.0, 5.0];
        let s = summary(&mut v).unwrap();
        assert!((s.mean - 3.0).abs() < 1e-12);
        assert!((s.stddev.unwrap() - 2.5_f64.sqrt()).abs() < 1e-12);
        assert_eq!((s.q1, s.median, s.q3), (2.0, 3.0, 4.0));
        // Even count: the median is between the two middle values.
        let s = summary(&mut [1.0, 2.0, 3.0, 4.0]).unwrap();
        assert!((s.median - 2.5).abs() < 1e-12);
        let one = summary(&mut [7.0]).unwrap();
        assert_eq!((one.stddev, one.median), (None, 7.0));
        assert_eq!(summary(&mut []), None);
    }

    #[test]
    fn the_iqr_rule_finds_the_outliers() {
        let mut v: Vec<f64> = (1..=10).map(f64::from).collect();
        v.extend([500.0, -300.0]);
        let s = summary(&mut v).unwrap();
        let o = outliers(&v, &s).unwrap();
        assert_eq!(o.count, 2);
        assert_eq!(o.examples, ["500", "-300"]);
        assert!(o.low_fence < 1.0 && o.high_fence < 500.0);
        assert!((o.share - 2.0 / 12.0).abs() < 1e-12);
    }

    #[test]
    fn no_outliers_when_there_are_none_or_it_cannot_be_told() {
        let mut v: Vec<f64> = (1..=10).map(f64::from).collect();
        let s = summary(&mut v).unwrap();
        assert_eq!(outliers(&v, &s), None);
        // Too few numbers.
        let mut few = vec![1.0, 2.0, 1000.0];
        let s = summary(&mut few).unwrap();
        assert_eq!(outliers(&few, &s), None);
        // The middle half is one value: the rule would flag every other value.
        let mut flat = vec![0.0; 20];
        flat.push(9.0);
        let s = summary(&mut flat).unwrap();
        assert_eq!(outliers(&flat, &s), None);
    }

    #[test]
    fn examples_are_distinct_and_capped() {
        let mut v: Vec<f64> = (1..=100).map(f64::from).collect();
        v.extend([1000.0, 1000.0, 2000.0, 3000.0, 4000.0, 5000.0, 6000.0]);
        let s = summary(&mut v).unwrap();
        let o = outliers(&v, &s).unwrap();
        assert_eq!(o.count, 7);
        assert_eq!(o.examples, ["6000", "5000", "4000", "3000", "2000"]);
    }

    #[test]
    fn a_numeric_column() {
        let mut values: Vec<Option<i64>> = (1..=10).map(Some).collect();
        values.extend([Some(900), None]);
        let p = run(vec![("n", Arc::new(Int64Array::from(values)))]);
        assert_eq!(p.rows, 12);
        let c = &p.columns[0];
        assert_eq!(
            (c.kind, c.profile.nulls, c.profile.distinct),
            (ColumnKind::Number, 1, 11)
        );
        assert_eq!(c.profile.histogram.len(), 20);
        assert_eq!(c.numbers.as_ref().unwrap().median, 6.0);
        assert_eq!(c.outliers.as_ref().unwrap().examples, ["900"]);
        assert!(c.patterns.is_none() && c.lengths.is_none());
    }

    #[test]
    fn nan_and_infinity_stay_out_of_the_spread() {
        let values = Float64Array::from(vec![1.0, 2.0, 3.0, f64::NAN, f64::INFINITY]);
        let p = run(vec![("f", Arc::new(values))]);
        let s = p.columns[0].numbers.as_ref().unwrap();
        assert!((s.mean - 2.0).abs() < 1e-12);
    }

    #[test]
    fn a_text_column() {
        let values = StringArray::from(vec![
            Some("ada@example.com"),
            Some("bob@example.com"),
            Some("carol@example.com"),
            Some("dave@example.com"),
            Some("not an email"),
            Some(""),
            None,
        ]);
        let p = run(vec![("email", Arc::new(values))]);
        let c = &p.columns[0];
        assert_eq!(c.kind, ColumnKind::Text);
        assert_eq!((c.profile.rows, c.profile.nulls, c.empty), (7, 1, 1));
        let l = c.lengths.as_ref().unwrap();
        assert_eq!((l.min, l.max), (0, 17));
        let r = c.patterns.as_ref().unwrap();
        assert_eq!(r.checked, 5);
        assert_eq!(r.dominant, Some(Pattern::Email));
        assert_eq!(r.exceptions, ["not an email"]);
        assert!(c.numbers.is_none() && c.outliers.is_none());
        assert_eq!(c.profile.top.len(), 5);
    }

    #[test]
    fn numbers_and_dates_stored_as_text() {
        let nums = StringArray::from(vec!["10", "20", "30", "40", "50", "x"]);
        let dates = StringArray::from(vec![
            "2024-01-01",
            "2024-02-01",
            "2024-03-01",
            "2024-04-01",
            "2024-05-01",
            "2024-06-01",
        ]);
        let decimals = StringArray::from(vec!["1,5", "2,5", "3.5", "4,5", "5,5", "6,5"]);
        let p = run(vec![
            ("n", Arc::new(nums)),
            ("d", Arc::new(dates)),
            ("m", Arc::new(decimals)),
        ]);
        let pattern = |i: usize| p.columns[i].patterns.as_ref().unwrap().dominant;
        assert_eq!(pattern(0), Some(Pattern::Integer));
        assert_eq!(pattern(1), Some(Pattern::IsoDate));
        assert_eq!(pattern(2), Some(Pattern::Decimal));
        assert_eq!(p.columns[0].patterns.as_ref().unwrap().exceptions, ["x"]);
    }

    #[test]
    fn dates_have_a_range_and_a_histogram() {
        let days = Date32Array::from(vec![19_723, 19_753, 19_753]);
        let p = run(vec![("d", Arc::new(days))]);
        let c = &p.columns[0];
        assert_eq!(c.kind, ColumnKind::Time);
        assert_eq!(c.profile.min.as_deref(), Some("2024-01-01"));
        assert_eq!(c.profile.histogram.len(), 20);
        assert!(c.numbers.is_none() && c.patterns.is_none());
    }

    #[test]
    fn other_columns_are_only_counted_and_an_empty_table_is_fine() {
        let bytes = BinaryArray::from(vec![Some(b"x".as_ref()), None]);
        let p = run(vec![("b", Arc::new(bytes))]);
        assert_eq!(
            (p.columns[0].kind, p.columns[0].profile.distinct),
            (ColumnKind::Other, 1)
        );
        let schema = Schema::new(vec![
            Field::new("s", DataType::Utf8, true),
            Field::new("n", DataType::Int64, true),
        ]);
        let empty = profile_table(&schema, &[], &FormatOptions::default()).unwrap();
        assert_eq!(empty.rows, 0);
        assert_eq!(empty.columns.len(), 2);
        let c = &empty.columns[0];
        assert_eq!((c.profile.rows, c.empty, c.lengths.clone()), (0, 0, None));
        assert_eq!(c.patterns.as_ref().unwrap().checked, 0);
        assert_eq!(empty.columns[1].numbers, None);
    }

    #[test]
    fn the_report_serializes_for_the_ui() {
        let p = run(vec![(
            "s",
            Arc::new(StringArray::from(vec!["a"])) as ArrayRef,
        )]);
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["columns"][0]["kind"], "text");
        assert_eq!(json["columns"][0]["distinct"], 1);
        assert_eq!(json["columns"][0]["name"], "s");
    }
}
