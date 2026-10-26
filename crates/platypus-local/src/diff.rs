//! What changed between two runs of a statement.
//!
//! Rows are compared on their Arrow values, each encoded once with `arrow_row` into a compact
//! byte string that is hashed and compared without making text: only the rows the diff shows
//! are formatted.
//!
//! A run may not be loaded in full. A row missing from such a run may just not be loaded yet,
//! so rows found in one run only are counted only when the other run is complete: added rows
//! when the previous run was loaded in full, removed rows when the new one was. Changed rows
//! (rows of both runs with the same key) are told from whatever was loaded. A partial diff
//! never reports a difference that is not there; it may miss some.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

use arrow_array::{ArrayRef, RecordBatch, StringArray};
use arrow_cast::display::{ArrayFormatter, FormatOptions};
use arrow_row::{RowConverter, Rows, SortField};
use arrow_schema::{ArrowError, DataType, Schema};
use serde::Serialize;

use crate::{LocalError, values};

/// Rows of a diff shown at most; the counts go on.
pub const MAX_ROWS: usize = 10_000;

/// A row as its values are shown; `None` for NULL.
type Texts = Vec<Option<String>>;

/// The rows one run had loaded.
#[derive(Debug, Clone, Copy)]
pub struct Table<'a> {
    pub schema: &'a Schema,
    pub batches: &'a [RecordBatch],
    /// They are all of the run's rows.
    pub complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffRow {
    pub change: Change,
    /// The row as it is now; a removed row as it was.
    pub values: Vec<Option<String>>,
    /// A changed row: the columns that differ, with their previous value.
    pub before: Vec<(usize, Option<String>)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Diff {
    pub columns: Vec<String>,
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    /// Rows matched by key columns; otherwise by all their values.
    pub keyed: bool,
    /// The differences, at most `MAX_ROWS`.
    pub rows: Vec<DiffRow>,
    pub truncated: bool,
    /// The rows the previous run had loaded.
    pub before_rows: usize,
    /// The rows the new run had loaded.
    pub after_rows: usize,
    /// Both runs were loaded in full. Otherwise added rows are counted only when the previous
    /// run is complete, removed rows only when the new one is.
    pub complete: bool,
}

impl Diff {
    /// Counts a `change`, keeping the row `make` shows while there is room. `make` returning
    /// `None` means the row shows no difference after all: it is not counted.
    fn record(
        &mut self,
        change: Change,
        make: impl FnOnce() -> Result<Option<DiffRow>, ArrowError>,
    ) -> Result<(), ArrowError> {
        if self.rows.len() < MAX_ROWS {
            let Some(row) = make()? else {
                return Ok(());
            };
            self.rows.push(row);
        } else {
            self.truncated = true;
        }
        match change {
            Change::Added => self.added += 1,
            Change::Removed => self.removed += 1,
            Change::Changed => self.changed += 1,
        }
        Ok(())
    }
}

/// Same column names and types, in the same order.
#[must_use]
pub fn same_columns(a: &Schema, b: &Schema) -> bool {
    a.fields().len() == b.fields().len()
        && a.fields()
            .iter()
            .zip(b.fields())
            .all(|(x, y)| x.name() == y.name() && x.data_type() == y.data_type())
}

/// What changed from `before` to `after`, rows matched by the `key` columns when given, else
/// by all their values; values shown as `options` formats them.
pub fn diff(
    before: Table<'_>,
    after: Table<'_>,
    key: Option<&[usize]>,
    options: &FormatOptions<'_>,
) -> Result<Diff, LocalError> {
    if !same_columns(before.schema, after.schema) {
        return Err(LocalError::ColumnsDiffer);
    }
    let width = after.schema.fields().len();
    if let Some(&column) = key.and_then(|k| k.iter().find(|&&c| c >= width)) {
        return Err(LocalError::NoColumn(column));
    }
    let whole = Encoder::new(after.schema, (0..width).collect())?;
    let keys = key
        .map(|k| Encoder::new(after.schema, k.to_vec()))
        .transpose()?;
    let old = Run::new(before, &whole, keys.as_ref(), options)?;
    let new = Run::new(after, &whole, keys.as_ref(), options)?;
    let mut out = Diff {
        columns: after
            .schema
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect(),
        keyed: key.is_some(),
        before_rows: old.len(),
        after_rows: new.len(),
        complete: before.complete && after.complete,
        ..Diff::default()
    };
    compare(&old, &new, &mut out)?;
    Ok(out)
}

/// Pairs each new row with the first unpaired old row of the same key; see the module docs
/// for what a partial run leaves out.
fn compare(old: &Run<'_>, new: &Run<'_>, out: &mut Diff) -> Result<(), ArrowError> {
    let mut pending = Pending::new(old.keys());
    let new_keys = new.keys();
    for j in 0..new.len() {
        match pending.take(new_keys.row(j).data()) {
            None if old.complete => out.record(Change::Added, || {
                Ok(Some(DiffRow {
                    change: Change::Added,
                    values: new.shown.row(j)?,
                    before: Vec::new(),
                }))
            })?,
            // Without a key the pair has the same values; with one, the other values may differ.
            Some(i) if old.whole.row(i) != new.whole.row(j) => {
                out.record(Change::Changed, || changed(old, i, new, j))?;
            }
            // The same row, or one the partial previous run may just not have loaded.
            _ => {}
        }
    }
    if new.complete {
        for i in pending.left() {
            out.record(Change::Removed, || {
                Ok(Some(DiffRow {
                    change: Change::Removed,
                    values: old.shown.row(i)?,
                    before: Vec::new(),
                }))
            })?;
        }
    }
    Ok(())
}

/// Old row `i` as new row `j`: the columns whose shown value differs; `None` when they all
/// show the same (e.g. two NaN with other bits).
fn changed(
    old: &Run<'_>,
    i: usize,
    new: &Run<'_>,
    j: usize,
) -> Result<Option<DiffRow>, ArrowError> {
    let values = new.shown.row(j)?;
    let mut before = Vec::new();
    for (c, value) in values.iter().enumerate() {
        let was = old.shown.cell(i, c)?;
        if &was != value {
            before.push((c, was));
        }
    }
    Ok((!before.is_empty()).then_some(DiffRow {
        change: Change::Changed,
        values,
        before,
    }))
}

/// Encodes some columns of rows with one converter, so the rows of both runs compare.
struct Encoder {
    converter: RowConverter,
    columns: Vec<usize>,
    /// Columns of a type the row format lacks, encoded as the text shown.
    as_text: Vec<bool>,
}

impl Encoder {
    fn new(schema: &Schema, columns: Vec<usize>) -> Result<Self, ArrowError> {
        let mut as_text = Vec::with_capacity(columns.len());
        let mut fields = Vec::with_capacity(columns.len());
        for &c in &columns {
            let field = SortField::new(schema.field(c).data_type().clone());
            let text = !RowConverter::supports_fields(std::slice::from_ref(&field));
            as_text.push(text);
            fields.push(if text {
                SortField::new(DataType::Utf8)
            } else {
                field
            });
        }
        Ok(Self {
            converter: RowConverter::new(fields)?,
            columns,
            as_text,
        })
    }

    fn encode(
        &self,
        batches: &[RecordBatch],
        options: &FormatOptions<'_>,
    ) -> Result<Rows, ArrowError> {
        let count = batches.iter().map(RecordBatch::num_rows).sum();
        let mut rows = self.converter.empty_rows(count, 0);
        for batch in batches {
            let arrays = self
                .columns
                .iter()
                .zip(&self.as_text)
                .map(|(&c, &text)| {
                    let array = batch.column(c);
                    if text {
                        let texts = values::texts(array.as_ref(), options)?;
                        Ok(Arc::new(StringArray::from(texts)) as ArrayRef)
                    } else {
                        Ok(Arc::clone(array))
                    }
                })
                .collect::<Result<Vec<_>, ArrowError>>()?;
            self.converter.append(&mut rows, &arrays)?;
        }
        Ok(rows)
    }
}

/// A run's rows encoded for comparing, and shown on demand.
struct Run<'a> {
    whole: Rows,
    /// The key columns of each row, when rows are matched by key.
    keys: Option<Rows>,
    shown: Shown<'a>,
    complete: bool,
}

impl<'a> Run<'a> {
    fn new(
        table: Table<'a>,
        whole: &Encoder,
        keys: Option<&Encoder>,
        options: &'a FormatOptions<'a>,
    ) -> Result<Self, ArrowError> {
        Ok(Self {
            whole: whole.encode(table.batches, options)?,
            keys: keys.map(|k| k.encode(table.batches, options)).transpose()?,
            shown: Shown::new(table.batches, options)?,
            complete: table.complete,
        })
    }

    fn len(&self) -> usize {
        self.whole.num_rows()
    }

    /// What rows are matched by: the key columns, else all the values.
    fn keys(&self) -> &Rows {
        self.keys.as_ref().unwrap_or(&self.whole)
    }
}

/// A run's rows as the grid shows them, formatted only when asked.
struct Shown<'a> {
    batches: &'a [RecordBatch],
    /// For each batch, a formatter per column.
    formatters: Vec<Vec<ArrayFormatter<'a>>>,
    /// Row index of the first row of each batch.
    starts: Vec<usize>,
}

impl<'a> Shown<'a> {
    fn new(batches: &'a [RecordBatch], options: &'a FormatOptions<'a>) -> Result<Self, ArrowError> {
        let mut starts = Vec::with_capacity(batches.len());
        let mut rows = 0;
        let mut formatters = Vec::with_capacity(batches.len());
        for batch in batches {
            starts.push(rows);
            rows += batch.num_rows();
            formatters.push(
                batch
                    .columns()
                    .iter()
                    .map(|c| ArrayFormatter::try_new(c.as_ref(), options))
                    .collect::<Result<_, _>>()?,
            );
        }
        Ok(Self {
            batches,
            formatters,
            starts,
        })
    }

    /// The batch holding row `i`, and the row's index in it.
    fn locate(&self, i: usize) -> (usize, usize) {
        let batch = self.starts.partition_point(|&s| s <= i).saturating_sub(1);
        (batch, i - self.starts[batch])
    }

    fn cell(&self, i: usize, column: usize) -> Result<Option<String>, ArrowError> {
        let (batch, row) = self.locate(i);
        if self.batches[batch].column(column).is_null(row) {
            return Ok(None);
        }
        self.formatters[batch][column]
            .value(row)
            .try_to_string()
            .map(Some)
    }

    fn row(&self, i: usize) -> Result<Texts, ArrowError> {
        (0..self.formatters.first().map_or(0, Vec::len))
            .map(|c| self.cell(i, c))
            .collect()
    }
}

/// The old rows not paired yet, by key, each key's rows in order.
struct Pending<'a> {
    /// For each key, its first unpaired row (`NONE` when all are paired) and its last row.
    chains: HashMap<&'a [u8], (usize, usize)>,
    /// The next row with the same key, `NONE` after the last.
    next: Vec<usize>,
    paired: Vec<bool>,
}

const NONE: usize = usize::MAX;

impl<'a> Pending<'a> {
    fn new(keys: &'a Rows) -> Self {
        let count = keys.num_rows();
        let mut chains: HashMap<&[u8], (usize, usize)> = HashMap::new();
        let mut next = vec![NONE; count];
        for (i, key) in keys.iter().enumerate() {
            match chains.entry(key.data()) {
                Entry::Vacant(e) => {
                    e.insert((i, i));
                }
                Entry::Occupied(mut e) => {
                    let (_, last) = e.get_mut();
                    next[*last] = i;
                    *last = i;
                }
            }
        }
        Self {
            chains,
            next,
            paired: vec![false; count],
        }
    }

    /// Pairs the first unpaired old row with `key`, if any.
    fn take(&mut self, key: &[u8]) -> Option<usize> {
        let (first, _) = self.chains.get_mut(key)?;
        let i = *first;
        if i == NONE {
            return None;
        }
        *first = self.next[i];
        self.paired[i] = true;
        Some(i)
    }

    /// The old rows left without a pair, in order.
    fn left(&self) -> impl Iterator<Item = usize> + '_ {
        self.paired
            .iter()
            .enumerate()
            .filter(|(_, paired)| !**paired)
            .map(|(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use arrow_array::{Float64Array, Int32Array};
    use arrow_schema::Field;

    use super::*;

    fn table(rows: &[(i32, &str)]) -> (Schema, Vec<RecordBatch>) {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, true),
        ]);
        let ids: ArrayRef = Arc::new(Int32Array::from(
            rows.iter().map(|r| r.0).collect::<Vec<_>>(),
        ));
        let names: ArrayRef = Arc::new(StringArray::from(
            rows.iter()
                .map(|r| (r.1 != "NULL").then_some(r.1))
                .collect::<Vec<_>>(),
        ));
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![ids, names]).unwrap();
        (schema, vec![batch])
    }

    fn compare_runs(
        before: (&[(i32, &str)], bool),
        after: (&[(i32, &str)], bool),
        key: Option<&[usize]>,
    ) -> Diff {
        let (s1, b1) = table(before.0);
        let (s2, b2) = table(after.0);
        let before = Table {
            schema: &s1,
            batches: &b1,
            complete: before.1,
        };
        let after = Table {
            schema: &s2,
            batches: &b2,
            complete: after.1,
        };
        diff(before, after, key, &FormatOptions::default()).unwrap()
    }

    fn compare(before: &[(i32, &str)], after: &[(i32, &str)], key: Option<&[usize]>) -> Diff {
        compare_runs((before, true), (after, true), key)
    }

    #[test]
    fn by_key_tells_changed_rows() {
        let d = compare(
            &[(1, "a"), (2, "b"), (3, "c")],
            &[(1, "a"), (2, "B"), (4, "d")],
            Some(&[0]),
        );
        assert_eq!((d.added, d.removed, d.changed, d.keyed), (1, 1, 1, true));
        assert_eq!(d.columns, ["id", "name"]);
        assert_eq!((d.before_rows, d.after_rows, d.complete), (3, 3, true));
        let changed = d.rows.iter().find(|r| r.change == Change::Changed).unwrap();
        assert_eq!(changed.values, [Some("2".to_owned()), Some("B".to_owned())]);
        assert_eq!(changed.before, [(1, Some("b".to_owned()))]);
        assert_eq!(d.rows.last().map(|r| r.change), Some(Change::Removed));
    }

    #[test]
    fn nulls_differ_from_values() {
        let d = compare(
            &[(1, "NULL"), (2, "x")],
            &[(1, "x"), (2, "NULL")],
            Some(&[0]),
        );
        assert_eq!(d.changed, 2);
        assert_eq!(d.rows[0].before, [(1, None)]);
        assert_eq!(d.rows[1].values, [Some("2".to_owned()), None]);
    }

    #[test]
    fn without_a_key_rows_are_added_or_removed() {
        let d = compare(&[(1, "a"), (2, "b")], &[(1, "a"), (2, "B")], None);
        assert_eq!((d.added, d.removed, d.changed, d.keyed), (1, 1, 0, false));
    }

    #[test]
    fn duplicates_pair_one_by_one() {
        let d = compare(&[(1, "a"), (1, "a"), (2, "b")], &[(1, "a"), (2, "b")], None);
        assert_eq!((d.added, d.removed), (0, 1));
        let keyed = compare(&[(1, "a"), (1, "x")], &[(1, "a")], Some(&[0]));
        assert_eq!((keyed.added, keyed.removed, keyed.changed), (0, 1, 0));
        let removed = &keyed.rows[0];
        assert_eq!(
            removed.values[1].as_deref(),
            Some("x"),
            "the second one is left"
        );
    }

    #[test]
    fn the_same_rows_make_no_difference() {
        let d = compare(&[(1, "a"), (2, "b")], &[(2, "b"), (1, "a")], Some(&[0]));
        assert_eq!((d.added, d.removed, d.changed), (0, 0, 0));
        assert_eq!(d.rows, Vec::<DiffRow>::new());
    }

    #[test]
    fn a_partial_run_reports_no_false_differences() {
        // The new run loaded only its first rows: the old rows it lacks are not "removed".
        let old: Vec<(i32, &str)> = (0..10).map(|i| (i, "a")).collect();
        let new: Vec<(i32, &str)> = vec![(5, "a"), (1, "B"), (42, "n")];
        let d = compare_runs((&old, true), (&new, false), Some(&[0]));
        assert_eq!((d.added, d.removed, d.changed), (1, 0, 1));
        assert_eq!((d.before_rows, d.after_rows, d.complete), (10, 3, false));
        // The old run was partial: a new row it lacks is not "added", the removed ones count.
        let d = compare_runs((&new, false), (&old, true), Some(&[0]));
        assert_eq!((d.added, d.removed, d.changed), (0, 1, 1));
        // Both partial and matched by all values: only one-sided rows, none can be told.
        let d = compare_runs((&old[..3], false), (&old[2..], false), None);
        assert_eq!((d.added, d.removed, d.changed), (0, 0, 0));
        assert!(!d.complete);
    }

    #[test]
    fn the_rows_shown_are_capped() {
        let after: Vec<(i32, &str)> = (0..10_005).map(|i| (i, "n")).collect();
        let d = compare(&[], &after, Some(&[0]));
        assert_eq!(d.added, 10_005);
        assert_eq!(d.rows.len(), MAX_ROWS);
        assert!(d.truncated);
    }

    /// `n` rows of `(id, id * factor)` in batches of 500, as a result's buffer holds them.
    fn numbers(n: i32, factor: f64) -> (Schema, Vec<RecordBatch>) {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("x", DataType::Float64, false),
        ]);
        let batches = (0..n)
            .step_by(500)
            .map(|from| {
                let ids: Vec<i32> = (from..(from + 500).min(n)).collect();
                let xs: Vec<f64> = ids.iter().map(|&i| f64::from(i) * factor).collect();
                let columns: Vec<ArrayRef> = vec![
                    Arc::new(Int32Array::from(ids)),
                    Arc::new(Float64Array::from(xs)),
                ];
                RecordBatch::try_new(Arc::new(schema.clone()), columns).unwrap()
            })
            .collect();
        (schema, batches)
    }

    #[test]
    fn large_runs_compare_quickly_and_show_only_the_cap() {
        let (schema, old) = numbers(200_000, 1.0);
        let (_, new) = numbers(200_000, 2.0);
        let run = |batches| Table {
            schema: &schema,
            batches,
            complete: true,
        };
        let started = Instant::now();
        let d = diff(run(&old), run(&new), Some(&[0]), &FormatOptions::default()).unwrap();
        // Every row but id 0 changed; only the first MAX_ROWS are formatted and kept.
        assert_eq!((d.added, d.removed, d.changed), (0, 0, 199_999));
        assert_eq!(d.rows.len(), MAX_ROWS);
        assert!(d.truncated);
        assert_eq!(d.rows[0].values[0].as_deref(), Some("1"));
        assert_eq!(d.rows[0].before, [(1, Some("1.0".to_owned()))]);
        let same = diff(run(&old), run(&old), None, &FormatOptions::default()).unwrap();
        assert_eq!((same.added, same.removed, same.rows.len()), (0, 0, 0));
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn other_columns_or_keys_are_errors() {
        let (schema, batches) = table(&[(1, "a")]);
        let other = Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, true),
        ]);
        let options = FormatOptions::default();
        let this = Table {
            schema: &schema,
            batches: &batches,
            complete: true,
        };
        let that = Table {
            schema: &other,
            batches: &[],
            complete: true,
        };
        assert!(!same_columns(&schema, &other));
        assert!(matches!(
            diff(this, that, None, &options),
            Err(LocalError::ColumnsDiffer)
        ));
        assert!(matches!(
            diff(this, this, Some(&[5]), &options),
            Err(LocalError::NoColumn(5))
        ));
    }
}
