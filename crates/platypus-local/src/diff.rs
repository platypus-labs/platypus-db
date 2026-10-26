//! What changed between two runs of a statement.

use std::collections::{HashMap, VecDeque};

use arrow_array::RecordBatch;
use arrow_cast::display::FormatOptions;
use arrow_schema::Schema;
use serde::Serialize;

use crate::{LocalError, values};

/// Rows of a diff shown at most; the counts go on.
pub const MAX_ROWS: usize = 10_000;

/// A row as its values are shown; `None` for NULL.
type Row = Vec<Option<String>>;

/// The rows of one run.
#[derive(Debug, Clone, Copy)]
pub struct Table<'a> {
    pub schema: &'a Schema,
    pub batches: &'a [RecordBatch],
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
}

impl Diff {
    fn push(&mut self, change: Change, values: Row, before: Vec<(usize, Option<String>)>) {
        match change {
            Change::Added => self.added += 1,
            Change::Removed => self.removed += 1,
            Change::Changed => self.changed += 1,
        }
        if self.rows.len() < MAX_ROWS {
            self.rows.push(DiffRow {
                change,
                values,
                before,
            });
        } else {
            self.truncated = true;
        }
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
/// by all their values; values compared as `options` shows them.
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
    let old = rows(before.batches, options)?;
    let new = rows(after.batches, options)?;
    let mut out = Diff {
        columns: after
            .schema
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect(),
        keyed: key.is_some(),
        ..Diff::default()
    };
    let mut left = match key {
        Some(key) => by_key(&old, &new, key, &mut out),
        None => by_row(&old, &new, &mut out),
    };
    left.sort_unstable();
    for i in left {
        out.push(Change::Removed, old[i].clone(), Vec::new());
    }
    Ok(out)
}

/// Every row's values as shown.
fn rows(batches: &[RecordBatch], options: &FormatOptions<'_>) -> Result<Vec<Row>, LocalError> {
    let mut out = Vec::with_capacity(batches.iter().map(RecordBatch::num_rows).sum());
    for batch in batches {
        let columns = batch
            .columns()
            .iter()
            .map(|c| values::texts(c.as_ref(), options))
            .collect::<Result<Vec<_>, _>>()?;
        for r in 0..batch.num_rows() {
            out.push(columns.iter().map(|c| c[r].clone()).collect());
        }
    }
    Ok(out)
}

/// Pairs the rows by `key`; returns the old rows left without a pair.
fn by_key(old: &[Row], new: &[Row], key: &[usize], out: &mut Diff) -> Vec<usize> {
    let key_of = |row: &Row| -> Row { key.iter().map(|&c| row[c].clone()).collect() };
    let mut pending: HashMap<Row, VecDeque<usize>> = HashMap::new();
    for (i, row) in old.iter().enumerate() {
        pending.entry(key_of(row)).or_default().push_back(i);
    }
    for row in new {
        match pending.get_mut(&key_of(row)).and_then(VecDeque::pop_front) {
            None => out.push(Change::Added, row.clone(), Vec::new()),
            Some(i) => {
                let before: Vec<(usize, Option<String>)> = old[i]
                    .iter()
                    .zip(row)
                    .enumerate()
                    .filter(|(_, (was, is))| was != is)
                    .map(|(c, (was, _))| (c, was.clone()))
                    .collect();
                if !before.is_empty() {
                    out.push(Change::Changed, row.clone(), before);
                }
            }
        }
    }
    pending.into_values().flatten().collect()
}

/// Pairs equal rows one by one; returns the old rows left without a pair.
fn by_row(old: &[Row], new: &[Row], out: &mut Diff) -> Vec<usize> {
    let mut pending: HashMap<&Row, VecDeque<usize>> = HashMap::new();
    for (i, row) in old.iter().enumerate() {
        pending.entry(row).or_default().push_back(i);
    }
    for row in new {
        if pending.get_mut(row).and_then(VecDeque::pop_front).is_none() {
            out.push(Change::Added, row.clone(), Vec::new());
        }
    }
    pending.into_values().flatten().collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Int32Array, StringArray};
    use arrow_schema::{DataType, Field};

    use super::*;

    fn table(rows: &[(i32, &str)]) -> (Schema, Vec<RecordBatch>) {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, false),
        ]);
        let ids: ArrayRef = Arc::new(Int32Array::from(
            rows.iter().map(|r| r.0).collect::<Vec<_>>(),
        ));
        let names: ArrayRef = Arc::new(StringArray::from(
            rows.iter().map(|r| r.1).collect::<Vec<_>>(),
        ));
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![ids, names]).unwrap();
        (schema, vec![batch])
    }

    fn compare(before: &[(i32, &str)], after: &[(i32, &str)], key: Option<&[usize]>) -> Diff {
        let (s1, b1) = table(before);
        let (s2, b2) = table(after);
        let before = Table {
            schema: &s1,
            batches: &b1,
        };
        let after = Table {
            schema: &s2,
            batches: &b2,
        };
        diff(before, after, key, &FormatOptions::default()).unwrap()
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
        let changed = d.rows.iter().find(|r| r.change == Change::Changed).unwrap();
        assert_eq!(changed.values, [Some("2".to_owned()), Some("B".to_owned())]);
        assert_eq!(changed.before, [(1, Some("b".to_owned()))]);
        assert_eq!(d.rows.last().map(|r| r.change), Some(Change::Removed));
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
    }

    #[test]
    fn the_same_rows_make_no_difference() {
        let d = compare(&[(1, "a"), (2, "b")], &[(2, "b"), (1, "a")], Some(&[0]));
        assert_eq!((d.added, d.removed, d.changed), (0, 0, 0));
        assert_eq!(d.rows, Vec::<DiffRow>::new());
    }

    #[test]
    fn the_rows_shown_are_capped() {
        let after: Vec<(i32, &str)> = (0..10_005).map(|i| (i, "n")).collect();
        let d = compare(&[], &after, Some(&[0]));
        assert_eq!(d.added, 10_005);
        assert_eq!(d.rows.len(), MAX_ROWS);
        assert!(d.truncated);
    }

    #[test]
    fn other_columns_or_keys_are_errors() {
        let (schema, batches) = table(&[(1, "a")]);
        let other = Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, false),
        ]);
        let options = FormatOptions::default();
        let this = Table {
            schema: &schema,
            batches: &batches,
        };
        let that = Table {
            schema: &other,
            batches: &[],
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
