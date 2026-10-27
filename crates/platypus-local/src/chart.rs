//! The data of a chart from a result's rows.

use std::collections::HashMap;

use arrow_array::{Array, RecordBatch};
use arrow_cast::display::FormatOptions;
use arrow_schema::{ArrowError, Schema};
use serde::{Deserialize, Serialize};

use crate::LocalError;
use crate::values::{self, Kind};

/// Points of a line or scatter series at most.
pub const MAX_POINTS: usize = 2_000;
/// Categories of bars and pie at most, besides `OTHER`.
pub const MAX_CATEGORIES: usize = 50;
/// The category that gathers the ones left out; the UI shows its own word for it.
pub const OTHER: &str = "Other";
/// A category: the text of its X values, `None` for NULL (apart from the text "NULL").
pub type Category = Option<String>;
/// The series that counts rows when no Y column is chosen.
const COUNT: &str = "count";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartKind {
    Bar,
    Line,
    Scatter,
    Pie,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aggregate {
    Sum,
    Count,
    Avg,
    Min,
    Max,
}

/// What to draw: the X column, the Y columns (one series each) and, for bars and pie, how the
/// rows of a category add up.
#[derive(Debug, Clone, Deserialize)]
pub struct ChartRequest {
    pub kind: ChartKind,
    pub x: usize,
    pub y: Vec<usize>,
    pub aggregate: Aggregate,
}

/// How the X values read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum XAxis {
    /// Categories (bars, pie) or, for lines and scatter, their position.
    Category,
    Number,
    /// Milliseconds since the epoch.
    Time,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Series {
    pub name: String,
    /// Bars and pie: one value per category.
    pub values: Vec<Option<f64>>,
    /// Lines and scatter: `[x, y]` points.
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChartData {
    pub x_axis: XAxis,
    /// The categories in the order they came; the last is `OTHER` when `other`.
    pub categories: Vec<Category>,
    pub other: bool,
    pub series: Vec<Series>,
    /// Rows the chart was computed from.
    pub rows: usize,
    /// Points were reduced or categories gathered to keep the chart light.
    pub reduced: bool,
}

/// What `request` draws from `batches`, X labels shown with `options`.
pub fn chart(
    schema: &Schema,
    batches: &[RecordBatch],
    request: &ChartRequest,
    options: &FormatOptions<'_>,
) -> Result<ChartData, LocalError> {
    let x = values::field(schema, request.x)?;
    let names = request
        .y
        .iter()
        .map(|&c| values::field(schema, c).map(|f| f.name().clone()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut data = match request.kind {
        ChartKind::Bar | ChartKind::Pie => by_category(batches, request, names, options)?,
        ChartKind::Line | ChartKind::Scatter => {
            by_point(batches, request, values::kind(x), names, options)?
        }
    };
    data.rows = batches.iter().map(RecordBatch::num_rows).sum();
    Ok(data)
}

/// The running aggregates of a category in one series.
#[derive(Debug, Clone, Copy, Default)]
struct Acc {
    count: usize,
    sum: f64,
    min: f64,
    max: f64,
}

impl Acc {
    fn add(&mut self, v: f64) {
        if self.count == 0 {
            self.min = v;
            self.max = v;
        } else {
            self.min = self.min.min(v);
            self.max = self.max.max(v);
        }
        self.count += 1;
        self.sum += v;
    }

    fn merge(&mut self, other: &Acc) {
        if other.count == 0 {
            return;
        }
        if self.count == 0 {
            *self = *other;
            return;
        }
        self.count += other.count;
        self.sum += other.sum;
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
    }

    /// The aggregate, `None` (no bar) when the category has no number to aggregate, as SQL's
    /// sum, avg, min and max are NULL there.
    fn value(&self, aggregate: Aggregate) -> Option<f64> {
        match aggregate {
            Aggregate::Count => Some(values::float(self.count)),
            _ if self.count == 0 => None,
            Aggregate::Sum => Some(self.sum),
            Aggregate::Avg => Some(self.sum / values::float(self.count)),
            Aggregate::Min => Some(self.min),
            Aggregate::Max => Some(self.max),
        }
    }
}

/// Bars and pie: one value per category and series, the categories in the order they come.
fn by_category(
    batches: &[RecordBatch],
    request: &ChartRequest,
    names: Vec<String>,
    options: &FormatOptions<'_>,
) -> Result<ChartData, LocalError> {
    // Without a Y column the one series counts rows.
    let (columns, names): (Vec<Option<usize>>, Vec<String>) = if request.y.is_empty() {
        (vec![None], vec![COUNT.to_owned()])
    } else {
        (request.y.iter().copied().map(Some).collect(), names)
    };
    let mut order: Vec<Category> = Vec::new();
    let mut index: HashMap<Category, usize> = HashMap::new();
    let mut accs: Vec<Vec<Acc>> = Vec::new();
    for batch in batches {
        let labels = values::texts(batch.column(request.x).as_ref(), options)?;
        let ys = columns
            .iter()
            .map(|c| c.map(|c| values::numbers(batch.column(c))).transpose())
            .collect::<Result<Vec<_>, ArrowError>>()?;
        for (row, label) in labels.into_iter().enumerate() {
            let at = *index.entry(label).or_insert_with_key(|label| {
                order.push(label.clone());
                accs.push(vec![Acc::default(); columns.len()]);
                order.len() - 1
            });
            for (acc, y) in accs[at].iter_mut().zip(&ys) {
                let value = match y {
                    None => Some(1.0),
                    Some(y) => y
                        .is_valid(row)
                        .then(|| y.value(row))
                        .filter(|v| v.is_finite()),
                };
                if let Some(v) = value {
                    acc.add(v);
                }
            }
        }
    }
    let (categories, accs, other) = fold(order, accs, request.aggregate);
    let series = names
        .into_iter()
        .enumerate()
        .map(|(s, name)| Series {
            name,
            values: accs.iter().map(|a| a[s].value(request.aggregate)).collect(),
            points: Vec::new(),
        })
        .collect();
    Ok(ChartData {
        x_axis: XAxis::Category,
        categories,
        other,
        series,
        rows: 0,
        reduced: other,
    })
}

/// Over `MAX_CATEGORIES` categories, the largest by the first series stay in their order and
/// the others gather in `OTHER`, last.
fn fold(
    order: Vec<Category>,
    accs: Vec<Vec<Acc>>,
    aggregate: Aggregate,
) -> (Vec<Category>, Vec<Vec<Acc>>, bool) {
    if order.len() <= MAX_CATEGORIES {
        return (order, accs, false);
    }
    let weight = |a: &[Acc]| {
        a.first()
            .and_then(|a| a.value(aggregate))
            .map_or(0.0, f64::abs)
    };
    let mut ranked: Vec<usize> = (0..order.len()).collect();
    ranked.sort_by(|&a, &b| weight(&accs[b]).total_cmp(&weight(&accs[a])));
    let mut keep = vec![false; order.len()];
    for &i in &ranked[..MAX_CATEGORIES] {
        keep[i] = true;
    }
    let mut rest = vec![Acc::default(); accs.first().map_or(0, Vec::len)];
    let mut categories = Vec::with_capacity(MAX_CATEGORIES + 1);
    let mut kept = Vec::with_capacity(MAX_CATEGORIES + 1);
    for ((label, acc), keep) in order.into_iter().zip(accs).zip(keep) {
        if keep {
            categories.push(label);
            kept.push(acc);
        } else {
            for (r, a) in rest.iter_mut().zip(&acc) {
                r.merge(a);
            }
        }
    }
    categories.push(Some(OTHER.to_owned()));
    kept.push(rest);
    (categories, kept, true)
}

/// Lines and scatter: the `[x, y]` points of each series, X as a number, milliseconds or the
/// position of its category.
fn by_point(
    batches: &[RecordBatch],
    request: &ChartRequest,
    x_kind: Kind,
    names: Vec<String>,
    options: &FormatOptions<'_>,
) -> Result<ChartData, LocalError> {
    let x_axis = match x_kind {
        Kind::Number => XAxis::Number,
        Kind::Time => XAxis::Time,
        Kind::Text | Kind::Opaque => XAxis::Category,
    };
    let mut categories: Vec<Category> = Vec::new();
    let mut index: HashMap<Category, usize> = HashMap::new();
    let mut points: Vec<Vec<[f64; 2]>> = vec![Vec::new(); request.y.len()];
    for batch in batches {
        let xs = x_values(
            batch,
            request.x,
            x_axis,
            &mut categories,
            &mut index,
            options,
        )?;
        for (series, &c) in points.iter_mut().zip(&request.y) {
            let ys = values::numbers(batch.column(c))?;
            series.extend(xs.iter().zip(ys.iter()).filter_map(|(x, y)| match (x, y) {
                (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Some([*x, y]),
                _ => None,
            }));
        }
    }
    let line = request.kind == ChartKind::Line;
    let mut reduced = false;
    let mut series: Vec<Series> = names
        .into_iter()
        .zip(points)
        .map(|(name, mut points)| {
            if line {
                points.sort_by(|a, b| a[0].total_cmp(&b[0]));
            }
            if points.len() > MAX_POINTS {
                points = reduce(&points);
                reduced = true;
            }
            Series {
                name,
                values: Vec::new(),
                points,
            }
        })
        .collect();
    if x_axis == XAxis::Category {
        categories = compact(&mut series, categories);
    }
    Ok(ChartData {
        x_axis,
        categories,
        other: false,
        series,
        rows: 0,
        reduced,
    })
}

/// The X of each row of `batch`: its number, or the position of its text among `categories`.
fn x_values(
    batch: &RecordBatch,
    column: usize,
    axis: XAxis,
    categories: &mut Vec<Category>,
    index: &mut HashMap<Category, usize>,
    options: &FormatOptions<'_>,
) -> Result<Vec<Option<f64>>, ArrowError> {
    if axis != XAxis::Category {
        return Ok(values::numbers(batch.column(column))?.iter().collect());
    }
    Ok(values::texts(batch.column(column).as_ref(), options)?
        .into_iter()
        .map(|label| {
            let at = *index.entry(label).or_insert_with_key(|label| {
                categories.push(label.clone());
                categories.len() - 1
            });
            Some(values::float(at))
        })
        .collect())
}

/// The categories some point is at, in their order, the points' X renumbered to match: a
/// reduced series sends the labels of the points it kept, not every text of the column.
fn compact(series: &mut [Series], categories: Vec<Category>) -> Vec<Category> {
    let position = |x: f64| usize::try_from(values::whole(x)).unwrap_or(0);
    let mut used = vec![false; categories.len()];
    for p in series.iter().flat_map(|s| &s.points) {
        if let Some(u) = used.get_mut(position(p[0])) {
            *u = true;
        }
    }
    let mut renumbered = vec![0; categories.len()];
    let mut kept = Vec::new();
    for (i, (label, used)) in categories.into_iter().zip(used).enumerate() {
        if used {
            renumbered[i] = kept.len();
            kept.push(label);
        }
    }
    for p in series.iter_mut().flat_map(|s| s.points.iter_mut()) {
        if let Some(&at) = renumbered.get(position(p[0])) {
            p[0] = values::float(at);
        }
    }
    kept
}

/// At most `MAX_POINTS`: the X range cut in `MAX_POINTS / 2` bands, of which the lowest and
/// the highest point stay, in X order.
#[must_use]
pub fn reduce(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let bands = MAX_POINTS / 2;
    let (lo, hi) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p[0]), hi.max(p[0]))
        });
    let width = (hi - lo) / values::float(bands);
    // The lowest and the highest point of each band, by index.
    let mut kept: Vec<Option<(usize, usize)>> = vec![None; bands];
    for (i, p) in points.iter().enumerate() {
        let band = if width > 0.0 {
            values::slot(p[0], lo, width, bands)
        } else {
            0
        };
        kept[band] = Some(match kept[band] {
            None => (i, i),
            Some((low, high)) => (
                if p[1] < points[low][1] { i } else { low },
                if p[1] > points[high][1] { i } else { high },
            ),
        });
    }
    let mut out = Vec::with_capacity(MAX_POINTS);
    for (low, high) in kept.into_iter().flatten() {
        let (first, second) = if points[low][0] <= points[high][0] {
            (low, high)
        } else {
            (high, low)
        };
        out.push(points[first]);
        if second != first {
            out.push(points[second]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Date32Array, Float64Array, Int32Array, StringArray};
    use arrow_schema::{DataType, Field};

    use super::*;

    /// `region` (text) and `amount` (float).
    fn cats(labels: &[&str]) -> Vec<Category> {
        labels.iter().map(|l| Some((*l).to_owned())).collect()
    }

    /// `k` (text) and `n` (float), NULL where `None`.
    fn keyed(rows: &[(Option<&str>, Option<f64>)]) -> (Schema, Vec<RecordBatch>) {
        let schema = Schema::new(vec![
            Field::new("k", DataType::Utf8, true),
            Field::new("n", DataType::Float64, true),
        ]);
        let columns: Vec<ArrayRef> = vec![
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.0).collect::<Vec<_>>(),
            )),
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.1).collect::<Vec<_>>(),
            )),
        ];
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), columns).unwrap();
        (schema, vec![batch])
    }

    #[test]
    fn null_is_not_the_text_null() {
        let (schema, batches) = keyed(&[
            (None, Some(1.0)),
            (Some("NULL"), Some(4.0)),
            (Some("a"), Some(2.0)),
        ]);
        let bars = draw(&schema, &batches, ChartKind::Bar, 0, &[1], Aggregate::Sum);
        assert_eq!(
            bars.categories,
            [None, Some("NULL".to_owned()), Some("a".to_owned())]
        );
        assert_eq!(bars.series[0].values, [Some(1.0), Some(4.0), Some(2.0)]);
        let line = draw(&schema, &batches, ChartKind::Line, 0, &[1], Aggregate::Sum);
        assert_eq!(line.categories.len(), 3);
    }

    #[test]
    fn a_category_without_numbers_has_no_value() {
        let (schema, batches) =
            keyed(&[(Some("a"), None), (Some("a"), None), (Some("b"), Some(3.0))]);
        for aggregate in [
            Aggregate::Sum,
            Aggregate::Avg,
            Aggregate::Min,
            Aggregate::Max,
        ] {
            let data = draw(&schema, &batches, ChartKind::Bar, 0, &[1], aggregate);
            assert_eq!(data.series[0].values, [None, Some(3.0)], "{aggregate:?}");
        }
        let count = draw(&schema, &batches, ChartKind::Bar, 0, &[1], Aggregate::Count);
        assert_eq!(count.series[0].values, [Some(0.0), Some(1.0)]);
    }

    fn sales(regions: &[&str], amounts: &[f64]) -> (Schema, Vec<RecordBatch>) {
        let schema = Schema::new(vec![
            Field::new("region", DataType::Utf8, true),
            Field::new("amount", DataType::Float64, true),
        ]);
        let columns: Vec<ArrayRef> = vec![
            Arc::new(StringArray::from(regions.to_vec())),
            Arc::new(Float64Array::from(amounts.to_vec())),
        ];
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), columns).unwrap();
        (schema, vec![batch])
    }

    fn draw(
        schema: &Schema,
        batches: &[RecordBatch],
        kind: ChartKind,
        x: usize,
        y: &[usize],
        aggregate: Aggregate,
    ) -> ChartData {
        let request = ChartRequest {
            kind,
            x,
            y: y.to_vec(),
            aggregate,
        };
        chart(schema, batches, &request, &FormatOptions::default()).unwrap()
    }

    #[test]
    fn bars_aggregate_by_category() {
        let (schema, batches) = sales(&["n", "s", "n"], &[1.0, 2.0, 4.0]);
        let sum = draw(&schema, &batches, ChartKind::Bar, 0, &[1], Aggregate::Sum);
        assert_eq!(sum.x_axis, XAxis::Category);
        assert_eq!(sum.categories, cats(&["n", "s"]));
        assert_eq!(sum.series[0].name, "amount");
        assert_eq!(sum.series[0].values, [Some(5.0), Some(2.0)]);
        assert_eq!(sum.rows, 3);
        assert!(!sum.reduced && !sum.other);
        let by = |aggregate| {
            draw(&schema, &batches, ChartKind::Bar, 0, &[1], aggregate).series[0]
                .values
                .clone()
        };
        assert_eq!(by(Aggregate::Avg), [Some(2.5), Some(2.0)]);
        assert_eq!(by(Aggregate::Min), [Some(1.0), Some(2.0)]);
        assert_eq!(by(Aggregate::Max), [Some(4.0), Some(2.0)]);
        assert_eq!(by(Aggregate::Count), [Some(2.0), Some(1.0)]);
        let rows = draw(&schema, &batches, ChartKind::Pie, 0, &[], Aggregate::Count);
        assert_eq!(rows.series[0].name, "count");
        assert_eq!(rows.series[0].values, [Some(2.0), Some(1.0)]);
    }

    #[test]
    fn categories_beyond_the_limit_are_gathered() {
        let names: Vec<String> = (0..60).map(|i| format!("c{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let amounts: Vec<f64> = (0..60).map(f64::from).collect();
        let (schema, batches) = sales(&refs, &amounts);
        let data = draw(&schema, &batches, ChartKind::Bar, 0, &[1], Aggregate::Sum);
        assert!(data.other && data.reduced);
        assert_eq!(data.categories.len(), MAX_CATEGORIES + 1);
        assert_eq!(data.categories.last(), Some(&Some(OTHER.to_owned())));
        // The 50 largest stay in their order (c10..c59); c0..c9 add up to 45.
        assert_eq!(data.categories[0].as_deref(), Some("c10"));
        assert_eq!(data.series[0].values.last().copied(), Some(Some(45.0)));
    }

    #[test]
    fn lines_follow_x_and_are_reduced() {
        let n = 10_000;
        let xs: Vec<i32> = (0..n).rev().collect();
        let ys: Vec<f64> = (0..n)
            .map(|i| if i == 1_234 { 1e6 } else { f64::from(i % 7) })
            .collect();
        let schema = Schema::new(vec![
            Field::new("x", DataType::Int32, false),
            Field::new("y", DataType::Float64, false),
        ]);
        let columns: Vec<ArrayRef> = vec![
            Arc::new(Int32Array::from(xs)),
            Arc::new(Float64Array::from(ys)),
        ];
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), columns).unwrap();
        let data = draw(&schema, &[batch], ChartKind::Line, 0, &[1], Aggregate::Sum);
        let points = &data.series[0].points;
        assert_eq!(data.x_axis, XAxis::Number);
        assert!(data.reduced);
        assert!(points.len() <= MAX_POINTS);
        assert!(points.windows(2).all(|w| w[0][0] <= w[1][0]), "in X order");
        assert!(points.iter().any(|p| p[1] > 999_999.0), "the peak stays");
    }

    #[test]
    fn small_series_keep_every_point() {
        let (schema, batches) = sales(&["a", "b", "c"], &[3.0, 1.0, 2.0]);
        let data = draw(
            &schema,
            &batches,
            ChartKind::Scatter,
            1,
            &[1],
            Aggregate::Sum,
        );
        assert_eq!(data.series[0].points.len(), 3);
        assert!(!data.reduced);
    }

    #[test]
    fn dates_on_x_are_milliseconds() {
        let schema = Schema::new(vec![
            Field::new("day", DataType::Date32, false),
            Field::new("n", DataType::Float64, false),
        ]);
        let columns: Vec<ArrayRef> = vec![
            Arc::new(Date32Array::from(vec![1, 0])),
            Arc::new(Float64Array::from(vec![2.0, 1.0])),
        ];
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), columns).unwrap();
        let data = draw(&schema, &[batch], ChartKind::Line, 0, &[1], Aggregate::Sum);
        assert_eq!(data.x_axis, XAxis::Time);
        assert_eq!(data.series[0].points, [[0.0, 1.0], [86_400_000.0, 2.0]]);
    }

    #[test]
    fn text_on_x_of_a_line_is_its_position() {
        let (schema, batches) = sales(&["b", "a", "b"], &[1.0, 2.0, 3.0]);
        let data = draw(&schema, &batches, ChartKind::Line, 0, &[1], Aggregate::Sum);
        assert_eq!(data.x_axis, XAxis::Category);
        assert_eq!(data.categories, cats(&["b", "a"]));
        assert_eq!(data.series[0].points, [[0.0, 1.0], [0.0, 3.0], [1.0, 2.0]]);
    }

    #[test]
    fn text_on_x_sends_only_the_labels_of_the_points_kept() {
        let names: Vec<String> = (0..10_000).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let amounts: Vec<f64> = (0..10_000).map(|i| f64::from(i % 13)).collect();
        let (schema, batches) = sales(&refs, &amounts);
        let data = draw(&schema, &batches, ChartKind::Line, 0, &[1], Aggregate::Sum);
        let points = &data.series[0].points;
        assert!(data.reduced);
        assert_eq!(data.categories.len(), points.len());
        for (i, p) in points.iter().enumerate() {
            assert_eq!(p[0], values::float(i), "renumbered in order");
        }
        assert_eq!(data.categories[0].as_deref(), Some("t0"));
        // A category without a point (its Y is NULL) gets no label.
        let (schema, batches) = sales(&["a", "b"], &[1.0, f64::NAN]);
        let data = draw(
            &schema,
            &batches,
            ChartKind::Scatter,
            0,
            &[1],
            Aggregate::Sum,
        );
        assert_eq!(data.categories, cats(&["a"]));
    }

    #[test]
    fn missing_columns_are_errors() {
        let (schema, batches) = sales(&["a"], &[1.0]);
        let request = |x, y: Vec<usize>| ChartRequest {
            kind: ChartKind::Bar,
            x,
            y,
            aggregate: Aggregate::Sum,
        };
        let options = FormatOptions::default();
        assert!(matches!(
            chart(&schema, &batches, &request(5, vec![1]), &options),
            Err(LocalError::NoColumn(5))
        ));
        assert!(matches!(
            chart(&schema, &batches, &request(0, vec![9]), &options),
            Err(LocalError::NoColumn(9))
        ));
    }
}
