//! Summaries of loaded rows: totals of a grid selection for the status line, statistics per
//! column for the calc panel, and counts by value for the grouping panel. All read the job's
//! buffered rows, off the UI thread.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::hash::{BuildHasher, RandomState};

use arrow_array::cast::AsArray;
use arrow_array::types::Float64Type;
use arrow_array::{Array, RecordBatch};
use arrow_cast::display::ArrayFormatter;
use arrow_schema::DataType;
use serde::Serialize;
use tauri::State;

use super::AppState;
use crate::display;
use crate::error::{CommandError, CommandResult};
use crate::jobs::Job;

/// Groups reported at most by [`group_counts`].
const GROUP_LIMIT: usize = 1_000;

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct SelectionStats {
    cells: usize,
    /// Cells that are not NULL.
    values: usize,
    /// Non-NULL cells of number columns, which the other totals are over.
    numbers: usize,
    sum: Option<f64>,
    avg: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
}

pub(super) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| CommandError::new("other", e.to_string()))?
}

fn check_columns(job: &Job, columns: &[usize]) -> CommandResult<()> {
    let (schema, _) = job
        .window(0, 0)
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    match columns.iter().find(|&&c| c >= schema.fields().len()) {
        Some(&c) => Err(CommandError::not_found("column", c + 1)),
        None => Ok(()),
    }
}

/// Loaded rows `offset..offset + limit`, once `columns` are known to exist.
pub(super) fn loaded_rows(
    job: &Job,
    offset: usize,
    limit: usize,
    columns: &[usize],
) -> CommandResult<Vec<RecordBatch>> {
    check_columns(job, columns)?;
    let (_, batches) = job
        .window(offset, limit)
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    Ok(batches)
}

fn stats_of(
    batches: &[RecordBatch],
    columns: &[usize],
) -> Result<SelectionStats, arrow_schema::ArrowError> {
    let mut s = SelectionStats::default();
    let (mut sum, mut min, mut max) = (0.0, f64::INFINITY, f64::NEG_INFINITY);
    for batch in batches {
        for &c in columns {
            let array = batch.column(c);
            s.cells += array.len();
            s.values += array.len() - array.null_count();
            if !array.data_type().is_numeric() {
                continue;
            }
            let numbers = arrow_cast::cast(array, &DataType::Float64)?;
            for v in numbers.as_primitive::<Float64Type>().iter().flatten() {
                s.numbers += 1;
                sum += v;
                min = min.min(v);
                max = max.max(v);
            }
        }
    }
    if s.numbers > 0 {
        s.sum = Some(sum);
        #[allow(clippy::cast_precision_loss)]
        let avg = sum / s.numbers as f64;
        s.avg = Some(avg);
        s.min = Some(min);
        s.max = Some(max);
    }
    Ok(s)
}

/// Statistics of one column over some rows, for the calc panel.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct ColumnStats {
    column: usize,
    /// Cells that are not NULL.
    values: usize,
    nulls: usize,
    /// Distinct non-NULL values, as the grid shows them.
    distinct: usize,
    /// For number columns.
    numbers: Option<NumberStats>,
    /// Smallest and largest text of other columns, as the grid shows it.
    first: Option<String>,
    last: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct NumberStats {
    sum: f64,
    avg: f64,
    min: f64,
    max: f64,
    median: f64,
    /// Sample standard deviation; `None` for a single value.
    stddev: Option<f64>,
}

fn column_stats_of(
    batches: &[RecordBatch],
    columns: &[usize],
) -> Result<Vec<ColumnStats>, arrow_schema::ArrowError> {
    let options = display::format_options();
    let mut out = Vec::with_capacity(columns.len());
    let mut text = String::new();
    let hasher = RandomState::new();
    for &c in columns {
        let mut s = ColumnStats {
            column: c,
            ..ColumnStats::default()
        };
        let numeric = batches
            .first()
            .is_some_and(|b| b.column(c).data_type().is_numeric());
        // Hashes, not copies of the values: a 64-bit collision is too rare to skew the count.
        let mut seen: HashSet<u64> = HashSet::new();
        let mut numbers: Vec<f64> = Vec::new();
        for batch in batches {
            let array = batch.column(c);
            s.nulls += array.null_count();
            s.values += array.len() - array.null_count();
            let formatter = ArrayFormatter::try_new(array.as_ref(), &options)?;
            for row in (0..array.len()).filter(|&r| !array.is_null(r)) {
                text.clear();
                let _ = write!(text, "{}", formatter.value(row));
                if !seen.insert(hasher.hash_one(&text)) || numeric {
                    continue;
                }
                if s.first.as_ref().is_none_or(|first| text < *first) {
                    s.first = Some(text.clone());
                }
                if s.last.as_ref().is_none_or(|last| text > *last) {
                    s.last = Some(text.clone());
                }
            }
            if numeric {
                let cast = arrow_cast::cast(array, &DataType::Float64)?;
                numbers.extend(cast.as_primitive::<Float64Type>().iter().flatten());
            }
        }
        s.distinct = seen.len();
        if numeric {
            s.numbers = number_stats(&mut numbers);
        }
        out.push(s);
    }
    Ok(out)
}

#[allow(clippy::cast_precision_loss)]
fn number_stats(values: &mut [f64]) -> Option<NumberStats> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable_by(f64::total_cmp);
    let n = values.len();
    let sum: f64 = values.iter().sum();
    let avg = sum / n as f64;
    let median = if n % 2 == 1 {
        values[n / 2]
    } else {
        f64::midpoint(values[n / 2 - 1], values[n / 2])
    };
    let stddev = (n > 1).then(|| {
        let squares: f64 = values.iter().map(|v| (v - avg).powi(2)).sum();
        (squares / (n - 1) as f64).sqrt()
    });
    Some(NumberStats {
        sum,
        avg,
        min: values[0],
        max: values[n - 1],
        median,
        stddev,
    })
}

/// Statistics of each of `columns` over loaded rows `offset..offset + limit`: counts and
/// distinct values, and for number columns sum, average, extremes, median and standard
/// deviation.
#[tauri::command]
pub async fn column_stats(
    state: State<'_, AppState>,
    job_id: u64,
    offset: usize,
    limit: usize,
    columns: Vec<usize>,
) -> CommandResult<Vec<ColumnStats>> {
    let job = state.job(job_id)?;
    blocking(move || {
        let batches = loaded_rows(&job, offset, limit, &columns)?;
        Ok(column_stats_of(&batches, &columns)?)
    })
    .await
}

/// Totals of loaded rows `offset..offset + limit` in `columns`: counts, and sum, average,
/// minimum and maximum of the number columns.
#[tauri::command]
pub async fn selection_stats(
    state: State<'_, AppState>,
    job_id: u64,
    offset: usize,
    limit: usize,
    columns: Vec<usize>,
) -> CommandResult<SelectionStats> {
    let job = state.job(job_id)?;
    blocking(move || {
        let batches = loaded_rows(&job, offset, limit, &columns)?;
        Ok(stats_of(&batches, &columns)?)
    })
    .await
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Group {
    /// One value per grouped column, as the grid shows it; `None` for NULL.
    values: Vec<Option<String>>,
    count: usize,
}

#[derive(Debug, Serialize)]
pub struct GroupCounts {
    /// Most frequent first, at most [`GROUP_LIMIT`].
    groups: Vec<Group>,
    /// Loaded rows counted.
    rows: usize,
    /// Distinct value combinations, also those left out of `groups`.
    distinct: usize,
}

fn groups(job: &Job, columns: &[usize]) -> CommandResult<GroupCounts> {
    check_columns(job, columns)?;
    let (_, batches) = job
        .snapshot()
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    Ok(groups_of(&batches, columns)?)
}

fn groups_of(
    batches: &[RecordBatch],
    columns: &[usize],
) -> Result<GroupCounts, arrow_schema::ArrowError> {
    let options = display::format_options();
    let mut counts: HashMap<Vec<Option<String>>, usize> = HashMap::new();
    let mut rows = 0;
    for batch in batches {
        let formatters = columns
            .iter()
            .map(|&c| ArrayFormatter::try_new(batch.column(c).as_ref(), &options))
            .collect::<Result<Vec<_>, _>>()?;
        for row in 0..batch.num_rows() {
            let key = columns
                .iter()
                .zip(&formatters)
                .map(|(&c, f)| {
                    (!batch.column(c).is_null(row)).then(|| {
                        let mut text = String::new();
                        let _ = write!(text, "{}", f.value(row));
                        text
                    })
                })
                .collect();
            *counts.entry(key).or_default() += 1;
        }
        rows += batch.num_rows();
    }
    let distinct = counts.len();
    let mut groups: Vec<Group> = counts
        .into_iter()
        .map(|(values, count)| Group { values, count })
        .collect();
    groups.sort_unstable_by(|a, b| b.count.cmp(&a.count).then_with(|| a.values.cmp(&b.values)));
    groups.truncate(GROUP_LIMIT);
    Ok(GroupCounts {
        groups,
        rows,
        distinct,
    })
}

/// How many loaded rows have each combination of values in `columns`.
#[tauri::command]
pub async fn group_counts(
    state: State<'_, AppState>,
    job_id: u64,
    columns: Vec<usize>,
) -> CommandResult<GroupCounts> {
    let job = state.job(job_id)?;
    blocking(move || groups(&job, &columns)).await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{Float64Array, Int32Array, StringArray};
    use arrow_schema::{Field, Schema};

    use super::*;

    fn batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("n", DataType::Int32, true),
            Field::new("x", DataType::Float64, true),
            Field::new("s", DataType::Utf8, true),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int32Array::from(vec![Some(1), None, Some(3), Some(3)])),
                Arc::new(Float64Array::from(vec![
                    Some(0.5),
                    Some(1.5),
                    None,
                    Some(2.0),
                ])),
                Arc::new(StringArray::from(vec![
                    Some("a"),
                    Some("b"),
                    Some("a"),
                    None,
                ])),
            ],
        )
        .unwrap()
    }

    #[test]
    fn totals_number_columns_only() {
        let s = stats_of(&[batch()], &[0, 1, 2]).unwrap();
        assert_eq!((s.cells, s.values, s.numbers), (12, 9, 6));
        assert_eq!(s.sum, Some(11.0));
        assert_eq!(s.min, Some(0.5));
        assert_eq!(s.max, Some(3.0));
        let text = stats_of(&[batch()], &[2]).unwrap();
        assert_eq!((text.values, text.sum), (3, None));
    }

    #[test]
    fn column_statistics() {
        let s = column_stats_of(&[batch(), batch()], &[0, 2]).unwrap();
        let n = &s[0];
        assert_eq!((n.column, n.values, n.nulls, n.distinct), (0, 6, 2, 2));
        let numbers = n.numbers.as_ref().unwrap();
        assert_eq!(
            (numbers.sum, numbers.min, numbers.max, numbers.median),
            (14.0, 1.0, 3.0, 3.0)
        );
        assert!((numbers.avg - 14.0 / 6.0).abs() < 1e-12);
        assert!((numbers.stddev.unwrap() - 1.032_795_558_988_644).abs() < 1e-9);
        assert_eq!(n.first, None);
        let text = &s[1];
        assert_eq!((text.values, text.nulls, text.distinct), (6, 2, 2));
        assert_eq!(
            (text.first.as_deref(), text.last.as_deref()),
            (Some("a"), Some("b"))
        );
        assert_eq!(text.numbers, None);
    }

    #[test]
    fn statistics_of_few_numbers() {
        assert_eq!(number_stats(&mut []), None);
        let one = number_stats(&mut [4.0]).unwrap();
        assert_eq!((one.median, one.stddev), (4.0, None));
        let even = number_stats(&mut [4.0, 1.0, 3.0, 2.0]).unwrap();
        assert_eq!(even.median, 2.5);
    }

    #[test]
    fn counts_value_combinations() {
        let g = groups_of(&[batch(), batch()], &[0, 2]).unwrap();
        assert_eq!((g.rows, g.distinct), (8, 4));
        let first = &g.groups[0];
        assert_eq!(first.count, 2);
        let pairs: Vec<_> = g
            .groups
            .iter()
            .map(|g| (g.values.clone(), g.count))
            .collect();
        assert!(pairs.contains(&(vec![Some("3".into()), Some("a".into())], 2)));
        assert!(pairs.contains(&(vec![None, Some("b".into())], 2)));
    }
}
