//! The rows that differ between two tables with the same key, possibly on two databases of
//! the same kind, and the statements that make the target's rows like the source's: rows
//! only in the source inserted, rows that differ updated (the changed columns only), rows
//! only in the target deleted when asked.
//!
//! Rows are matched by the source's primary key (or a unique key of non-null columns), which
//! the target must have too, and compared by their values as text: PostgreSQL writes each
//! value as its own text, the other databases as the data viewer shows it. Both sides are
//! read whole into memory, up to [`MAX_ROWS`] rows each; a filter narrows them.

use std::collections::HashMap;

use futures::StreamExt;
use platypus_core::{
    Connection, ConstraintKind, Error, ExecOptions, ExecResult, Result, TableDetails,
};
use platypus_sql::Dialect;
use platypus_sql::dml::{self, Column, Value, ValueKind};
use platypus_sql::generated::{Generated, generated_columns};
use serde::{Deserialize, Serialize};

use crate::import::rows_text;

/// Most rows read from each table.
pub const MAX_ROWS: usize = 500_000;

/// Most rows listed in [`DataDiff::samples`]; the counts and the statements cover them all.
pub const MAX_SAMPLES: usize = 200;

/// A table of one side of the comparison.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TableRef {
    pub schema: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct CompareOptions {
    /// A condition both tables are read with, e.g. `created_at >= '2026-01-01'`.
    #[serde(default)]
    pub filter: Option<String>,
    /// Whether the rows only in the target are deleted.
    #[serde(default)]
    pub delete_missing: bool,
}

type Row = Vec<Option<String>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// Only in the source: inserted.
    Added,
    /// Only in the target: deleted if asked.
    Removed,
    /// In both, with other values.
    Changed,
}

/// A column of a changed row, with its value on each side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CellChange {
    pub column: String,
    pub source: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RowChange {
    pub change: Change,
    /// The values of the key columns.
    pub key: Row,
    /// The columns that differ for a changed row; empty otherwise.
    pub cells: Vec<CellChange>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DataDiff {
    /// The key the rows are matched by.
    pub key: Vec<String>,
    /// The columns compared, the key first.
    pub columns: Vec<String>,
    pub same: u64,
    pub added: u64,
    pub removed: u64,
    pub changed: u64,
    /// The first [`MAX_SAMPLES`] differences, in key order of the source then of the target.
    pub samples: Vec<RowChange>,
    /// The statements that make the target like the source, to review before running.
    pub statements: Vec<String>,
    /// What the statements leave as it is or cannot do.
    pub warnings: Vec<String>,
}

/// How the rows of both tables are read and written: the columns compared, the key first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub columns: Vec<String>,
    /// How many of `columns`, from the first, are the key.
    pub key: usize,
    /// The target's columns that take a value only with an override (identities), by index.
    pub identity: Vec<usize>,
}

/// Compares `from` on `source` with `to` on `target`, both of `dialect`.
///
/// # Errors
///
/// When a table cannot be read, has no key the other has too, or has more than [`MAX_ROWS`]
/// rows.
pub async fn compare_data(
    source: &dyn Connection,
    target: &dyn Connection,
    dialect: Dialect,
    (from, to): (&TableRef, &TableRef),
    options: &CompareOptions,
) -> Result<DataDiff> {
    let (source_details, target_details) = futures::try_join!(
        source.table_details(&from.schema, &from.name),
        target.table_details(&to.schema, &to.name)
    )?;
    // A catalog the server does not have (an old version) leaves every column writable.
    let (source_generated, target_generated) = futures::join!(
        generated_columns(source, dialect, &from.schema, &from.name),
        generated_columns(target, dialect, &to.schema, &to.name)
    );
    let source_generated = source_generated.unwrap_or_default();
    let target_generated = target_generated.unwrap_or_default();
    let generated = |name: &str, kind: Generated| {
        source_generated
            .iter()
            .chain(&target_generated)
            .any(|g| g.name == name && g.generated == kind)
    };
    let (layout, mut warnings) = layout(&source_details, &target_details, |c| {
        generated(c, Generated::Computed)
    })?;
    let identity = layout
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            target_generated
                .iter()
                .any(|g| &g.name == *c && g.generated == Generated::Identity)
        })
        .map(|(i, _)| i)
        .collect();
    let layout = Layout { identity, ..layout };
    let ((source_rows, source_kinds), (target_rows, target_kinds)) = futures::try_join!(
        read(source, dialect, from, &layout, options),
        read(target, dialect, to, &layout, options)
    )?;
    if let Some(key) = duplicate_key(&target_rows, layout.key) {
        let shown: Vec<&str> = key.iter().map(|v| v.as_deref().unwrap_or("NULL")).collect();
        return Err(Error::Other(format!(
            "{} has several rows with the key ({}): the key does not tell its rows apart.",
            to.name,
            shown.join(", ")
        )));
    }
    let kinds = source_kinds.or(target_kinds).unwrap_or_default();
    let table = dialect.qualified(Some(&to.schema), &to.name);
    let mut diff = diff(
        dialect,
        &table,
        &layout,
        &kinds,
        (source_rows, target_rows),
        options.delete_missing,
    )
    .map_err(|e| Error::Other(e.to_string()))?;
    warnings.append(&mut diff.warnings);
    diff.warnings = warnings;
    Ok(diff)
}

/// The key and the columns compared: the source's primary key (or a unique key of non-null
/// columns), which the target must have, and the other columns of both tables that the
/// database does not compute. The warnings name the columns left out.
///
/// # Errors
///
/// When the source has no such key, or the target lacks one of its columns.
pub fn layout(
    source: &TableDetails,
    target: &TableDetails,
    computed: impl Fn(&str) -> bool,
) -> Result<(Layout, Vec<String>)> {
    let key = key_of(source).ok_or_else(|| {
        Error::Other(
            "The source table has no primary key nor a unique key of non-null columns to match \
             rows by."
                .to_owned(),
        )
    })?;
    let in_target = |name: &str| target.columns.iter().any(|c| c.name == name);
    if let Some(missing) = key.iter().find(|k| !in_target(k)) {
        return Err(Error::Other(format!(
            "The target table has no column {missing}, of the key the rows are matched by."
        )));
    }
    let mut warnings = Vec::new();
    let mut columns = key.clone();
    for c in &source.columns {
        if key.contains(&c.name) {
            continue;
        }
        if !in_target(&c.name) {
            warnings.push(format!(
                "The column {} is only in the source: it is not compared.",
                c.name
            ));
        } else if computed(&c.name) {
            warnings.push(format!(
                "The column {} is computed by the database: it is not compared.",
                c.name
            ));
        } else {
            columns.push(c.name.clone());
        }
    }
    for c in &target.columns {
        if !source.columns.iter().any(|s| s.name == c.name) {
            warnings.push(format!(
                "The column {} is only in the target: it is left as it is.",
                c.name
            ));
        }
    }
    Ok((
        Layout {
            key: key.len(),
            columns,
            identity: Vec::new(),
        },
        warnings,
    ))
}

/// The columns of the primary key, or else of a unique constraint or index whose columns
/// are all non-null.
fn key_of(details: &TableDetails) -> Option<Vec<String>> {
    if let Some(pk) = details
        .constraints
        .iter()
        .find(|c| c.kind == ConstraintKind::PrimaryKey && !c.columns.is_empty())
    {
        return Some(pk.columns.clone());
    }
    let pk: Vec<String> = details
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.clone())
        .collect();
    if !pk.is_empty() {
        return Some(pk);
    }
    let not_null = |names: &[String]| {
        !names.is_empty()
            && names
                .iter()
                .all(|n| details.columns.iter().any(|c| &c.name == n && !c.nullable))
    };
    details
        .constraints
        .iter()
        .filter(|c| c.kind == ConstraintKind::Unique)
        .map(|c| &c.columns)
        .chain(
            details
                .indexes
                .iter()
                .filter(|i| i.unique)
                .map(|i| &i.columns),
        )
        .find(|columns| not_null(columns))
        .cloned()
}

/// The rows of `table`, its columns in the order of `layout`, sorted by the key, and how
/// their values are written (`None` without rows).
async fn read(
    conn: &dyn Connection,
    dialect: Dialect,
    table: &TableRef,
    layout: &Layout,
    options: &CompareOptions,
) -> Result<(Vec<Row>, Option<Vec<ValueKind>>)> {
    let quoted: Vec<_> = layout
        .columns
        .iter()
        .map(|c| dialect.quote_ident(c))
        .collect();
    // PostgreSQL writes every value as its own text, which it reads back as the column's
    // type: arrays, ranges, money and the rest included (as the SQL dump does).
    let select: Vec<String> = if dialect == Dialect::Postgres {
        quoted.iter().map(|q| format!("{q}::text")).collect()
    } else {
        quoted.iter().map(ToString::to_string).collect()
    };
    let filter = options
        .filter
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(|f| format!(" WHERE {f}"))
        .unwrap_or_default();
    let sql = format!(
        // A PostgreSQL table's rows with those of its partitions and children, as a query
        // reads them: a partitioned table holds none of its own.
        "SELECT {} FROM {}{filter} ORDER BY {}",
        select.join(", "),
        dialect.qualified(Some(&table.schema), &table.name),
        quoted[..layout.key].join(", ")
    );
    let ExecResult::Rows(mut stream) = conn.execute(&sql, &ExecOptions::default()).await? else {
        return Ok((Vec::new(), None));
    };
    let mut rows = Vec::new();
    let mut kinds = None;
    while let Some(batch) = stream.next().await {
        let batch = batch?;
        if kinds.is_none() && batch.num_rows() > 0 {
            kinds = Some(
                batch
                    .schema()
                    .fields()
                    .iter()
                    .map(|f| {
                        if dialect == Dialect::Postgres {
                            ValueKind::Text
                        } else {
                            ValueKind::of(f.data_type())
                        }
                    })
                    .collect(),
            );
        }
        rows.extend(rows_text(&batch).map_err(|e| Error::Other(e.to_string()))?);
        if rows.len() > MAX_ROWS {
            return Err(Error::Other(format!(
                "{} has more than {MAX_ROWS} rows to compare: add a filter.",
                table.name
            )));
        }
    }
    Ok((rows, kinds))
}

/// The first key of the `key` first columns that two of `rows` share: the target is matched
/// by the source's key, which it may not have as a key of its own.
fn duplicate_key(rows: &[Row], key: usize) -> Option<Row> {
    let mut seen = std::collections::HashSet::with_capacity(rows.len());
    rows.iter()
        .map(|r| &r[..key])
        .find(|k| !seen.insert(*k))
        .map(<[Option<String>]>::to_vec)
}

/// The differences between the `source` and `target` rows of `layout` and the statements
/// that make `table` (quoted, the target) like the source; `kinds` says how the values are
/// written.
///
/// # Errors
///
/// When a value is not one of its column's kind.
pub fn diff(
    dialect: Dialect,
    table: &str,
    layout: &Layout,
    kinds: &[ValueKind],
    (source, target): (Vec<Row>, Vec<Row>),
    delete_missing: bool,
) -> std::result::Result<DataDiff, dml::InvalidValue> {
    let columns: Vec<Column<'_>> = layout
        .columns
        .iter()
        .enumerate()
        .map(|(i, name)| Column {
            name,
            kind: kinds.get(i).copied().unwrap_or(ValueKind::Text),
            default: None,
        })
        .collect();
    let key_of = |row: &Row| row[..layout.key].to_vec();
    let mut targets: HashMap<Row, Row> = target.into_iter().map(|r| (key_of(&r), r)).collect();
    let mut out = DataDiff {
        key: layout.columns[..layout.key].to_vec(),
        columns: layout.columns.clone(),
        ..DataDiff::default()
    };
    let mut added = Vec::new();
    let mut updates = Vec::new();
    let mut identity_changed = false;
    for row in source {
        let key = key_of(&row);
        let Some(old) = targets.remove(&key) else {
            out.added += 1;
            sample(&mut out, Change::Added, key, Vec::new());
            added.push(row);
            continue;
        };
        let differ: Vec<usize> = (layout.key..row.len())
            .filter(|&i| row[i] != old[i])
            .collect();
        if differ.is_empty() {
            out.same += 1;
            continue;
        }
        out.changed += 1;
        let cells = differ
            .iter()
            .map(|&i| CellChange {
                column: layout.columns[i].clone(),
                source: row[i].clone(),
                target: old[i].clone(),
            })
            .collect();
        sample(&mut out, Change::Changed, key.clone(), cells);
        // An identity column cannot be changed.
        let set: Vec<usize> = differ
            .into_iter()
            .filter(|i| !layout.identity.contains(i))
            .collect();
        identity_changed |= set.is_empty();
        if !set.is_empty() {
            updates.push((row, set));
        }
    }
    // The rows only in the target, in key order as read.
    let mut removed: Vec<Row> = targets.into_keys().collect();
    removed.sort();
    remove(&mut out, dialect, table, &columns, &removed, delete_missing)?;
    for (row, set) in &updates {
        let values: Vec<(usize, Value<'_>)> =
            set.iter().map(|&i| (i, value(row[i].as_ref()))).collect();
        let key: Vec<(usize, Option<&str>)> =
            (0..layout.key).map(|i| (i, row[i].as_deref())).collect();
        out.statements
            .push(dml::update(dialect, table, &columns, &values, &key)?);
    }
    if identity_changed {
        out.warnings.push(
            "Rows whose only difference is in an identity column are not updated: the \
             database does not change those."
                .to_owned(),
        );
    }
    let inserts: Vec<Vec<(usize, Value<'_>)>> = added
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(i, v)| (i, value(v.as_ref())))
                .collect()
        })
        .collect();
    out.statements.extend(dml::restore_inserts(
        dialect,
        table,
        &columns,
        &inserts,
        &layout.identity,
    )?);
    Ok(out)
}

/// Counts the rows only in the target (their `keys`) and deletes them if asked.
fn remove(
    out: &mut DataDiff,
    dialect: Dialect,
    table: &str,
    columns: &[Column<'_>],
    keys: &[Row],
    delete_missing: bool,
) -> std::result::Result<(), dml::InvalidValue> {
    out.removed = keys.len() as u64;
    for key in keys {
        sample(out, Change::Removed, key.clone(), Vec::new());
    }
    if delete_missing {
        let keys: Vec<Vec<(usize, Option<&str>)>> = keys
            .iter()
            .map(|k| {
                k.iter()
                    .enumerate()
                    .map(|(i, v)| (i, v.as_deref()))
                    .collect()
            })
            .collect();
        for s in dml::deletes(dialect, table, columns, &keys)? {
            out.statements.push(s.sql);
        }
    } else if !keys.is_empty() {
        out.warnings
            .push(format!("{} rows only in the target are kept.", keys.len()));
    }
    Ok(())
}

fn value(v: Option<&String>) -> Value<'_> {
    v.map_or(Value::Null, |s| Value::Text(s))
}

fn sample(out: &mut DataDiff, change: Change, key: Row, cells: Vec<CellChange>) {
    if out.samples.len() < MAX_SAMPLES {
        out.samples.push(RowChange { change, key, cells });
    }
}

#[cfg(test)]
mod tests {
    use platypus_core::{ColumnMeta, ConstraintMeta};

    use super::*;

    fn row(values: &[Option<&str>]) -> Row {
        values.iter().map(|v| v.map(str::to_owned)).collect()
    }

    fn layout(columns: &[&str], key: usize) -> Layout {
        Layout {
            columns: columns.iter().map(|c| (*c).to_owned()).collect(),
            key,
            identity: Vec::new(),
        }
    }

    fn column(name: &str, nullable: bool, primary_key: bool) -> ColumnMeta {
        ColumnMeta {
            name: name.to_owned(),
            data_type: "integer".to_owned(),
            nullable,
            default: None,
            primary_key,
            comment: None,
            enum_values: Vec::new(),
            fields: Vec::new(),
            hidden: false,
        }
    }

    const KINDS: [ValueKind; 3] = [ValueKind::Integer, ValueKind::Text, ValueKind::Integer];

    #[test]
    fn inserts_updates_and_deletes_to_make_the_target_like_the_source() {
        let source = vec![
            row(&[Some("1"), Some("a"), Some("10")]),
            row(&[Some("2"), Some("b"), Some("20")]),
            row(&[Some("3"), Some("c"), None]),
        ];
        let target = vec![
            row(&[Some("1"), Some("a"), Some("10")]),
            row(&[Some("2"), Some("B"), Some("20")]),
            row(&[Some("4"), Some("d"), Some("40")]),
        ];
        let d = diff(
            Dialect::Sqlite,
            "\"t\"",
            &layout(&["id", "name", "n"], 1),
            &KINDS,
            (source, target),
            true,
        )
        .unwrap();
        assert_eq!((d.same, d.added, d.changed, d.removed), (1, 1, 1, 1));
        assert_eq!(
            d.statements,
            [
                "DELETE FROM \"t\" WHERE id = 4",
                "UPDATE \"t\" SET name = 'b' WHERE id = 2",
                "INSERT INTO \"t\" (id, name, n) VALUES (3, 'c', NULL)",
            ]
        );
        assert_eq!(
            d.samples
                .iter()
                .map(|s| (s.change, s.key[0].as_deref().unwrap()))
                .collect::<Vec<_>>(),
            [
                (Change::Changed, "2"),
                (Change::Added, "3"),
                (Change::Removed, "4")
            ]
        );
        assert_eq!(
            d.samples[0].cells,
            [CellChange {
                column: "name".to_owned(),
                source: Some("b".to_owned()),
                target: Some("B".to_owned()),
            }]
        );
        assert_eq!(d.warnings, Vec::<String>::new());
    }

    #[test]
    fn rows_only_in_the_target_are_kept_unless_asked() {
        let d = diff(
            Dialect::Postgres,
            "t",
            &layout(&["id", "name", "n"], 1),
            &KINDS,
            (Vec::new(), vec![row(&[Some("4"), Some("d"), None])]),
            false,
        )
        .unwrap();
        assert_eq!(d.removed, 1);
        assert_eq!(d.statements, Vec::<String>::new());
        assert_eq!(d.warnings, ["1 rows only in the target are kept."]);
    }

    #[test]
    fn composite_keys_and_null_values_match() {
        let rows = || {
            vec![
                row(&[Some("1"), Some("x"), None]),
                row(&[Some("1"), Some("y"), Some("5")]),
            ]
        };
        let d = diff(
            Dialect::MySql,
            "t",
            &layout(&["a", "b", "n"], 2),
            &[ValueKind::Integer, ValueKind::Text, ValueKind::Integer],
            (rows(), rows()),
            true,
        )
        .unwrap();
        assert_eq!((d.same, d.added, d.changed, d.removed), (2, 0, 0, 0));
        assert_eq!(d.statements, Vec::<String>::new());
    }

    #[test]
    fn identity_columns_are_inserted_with_an_override_and_never_updated() {
        let mut l = layout(&["id", "code", "seq"], 1);
        l.identity = vec![2];
        let d = diff(
            Dialect::SqlServer,
            "[t]",
            &l,
            &KINDS,
            (
                vec![
                    row(&[Some("1"), Some("a"), Some("7")]),
                    row(&[Some("2"), Some("b"), Some("8")]),
                ],
                vec![row(&[Some("1"), Some("a"), Some("9")])],
            ),
            false,
        )
        .unwrap();
        assert_eq!(d.changed, 1);
        assert_eq!(
            d.statements,
            [
                "SET IDENTITY_INSERT [t] ON",
                "INSERT INTO [t] (id, code, seq) VALUES (2, N'b', 8)",
                "SET IDENTITY_INSERT [t] OFF",
            ]
        );
        assert_eq!(d.warnings.len(), 1, "{:?}", d.warnings);
    }

    #[test]
    fn samples_stop_at_the_limit_but_counts_and_statements_do_not() {
        let source: Vec<Row> = (0..MAX_SAMPLES + 5)
            .map(|i| row(&[Some(&i.to_string()), Some("a"), None]))
            .collect();
        let d = diff(
            Dialect::Sqlite,
            "t",
            &layout(&["id", "name", "n"], 1),
            &KINDS,
            (source, Vec::new()),
            true,
        )
        .unwrap();
        assert_eq!(d.added, (MAX_SAMPLES + 5) as u64);
        assert_eq!(d.samples.len(), MAX_SAMPLES);
        assert_eq!(d.statements.len(), 1, "one batched INSERT");
    }

    #[test]
    fn a_key_shared_by_two_target_rows_is_found() {
        let rows = vec![
            row(&[Some("A"), Some("1")]),
            row(&[Some("B"), Some("1")]),
            row(&[Some("A"), Some("2")]),
        ];
        assert_eq!(duplicate_key(&rows, 1), Some(row(&[Some("A")])));
        assert_eq!(duplicate_key(&rows, 2), None);
    }

    #[test]
    fn a_value_not_of_its_kind_is_an_error() {
        let err = diff(
            Dialect::Sqlite,
            "t",
            &layout(&["id", "name", "n"], 1),
            &KINDS,
            (vec![row(&[Some("x"), Some("a"), None])], Vec::new()),
            true,
        )
        .unwrap_err();
        assert_eq!(err.column, "id");
    }

    #[test]
    fn the_key_is_the_primary_key_or_a_unique_key_of_non_null_columns() {
        let pk = TableDetails {
            columns: vec![column("id", false, true), column("code", false, false)],
            ..TableDetails::default()
        };
        assert_eq!(key_of(&pk).unwrap(), ["id"]);
        let unique = |nullable| TableDetails {
            columns: vec![column("code", nullable, false), column("n", true, false)],
            constraints: vec![ConstraintMeta {
                name: "u".to_owned(),
                kind: ConstraintKind::Unique,
                columns: vec!["code".to_owned()],
                definition: String::new(),
            }],
            ..TableDetails::default()
        };
        assert_eq!(key_of(&unique(false)).unwrap(), ["code"]);
        assert_eq!(key_of(&unique(true)), None);
    }

    #[test]
    fn the_layout_leaves_out_and_names_the_columns_not_on_both_sides() {
        let source = TableDetails {
            columns: vec![
                column("id", false, true),
                column("name", true, false),
                column("only_here", true, false),
                column("total", true, false),
            ],
            ..TableDetails::default()
        };
        let target = TableDetails {
            columns: vec![
                column("id", false, true),
                column("total", true, false),
                column("name", true, false),
                column("extra", true, false),
            ],
            ..TableDetails::default()
        };
        let (l, warnings) = super::layout(&source, &target, |c| c == "total").unwrap();
        assert_eq!(l.columns, ["id", "name"]);
        assert_eq!(l.key, 1);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        let keyless = TableDetails {
            columns: vec![column("code", false, false)],
            ..TableDetails::default()
        };
        assert!(super::layout(&keyless, &target, |_| false).is_err());
        // The target lacks the source's key.
        assert!(super::layout(&source, &keyless, |_| false).is_err());
    }
}
