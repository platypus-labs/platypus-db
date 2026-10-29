//! A snapshot taken before an `UPDATE` or `DELETE` run by hand: the rows it is about to
//! change, read on the same session (in its transaction), kept as the statements that put
//! them back. A `DELETE` comes back as `INSERT`s, an `UPDATE` as `UPDATE`s by primary key.

use futures::StreamExt;
use platypus_core::{Connection, ExecOptions, ExecResult};
use platypus_sql::Dialect;
use platypus_sql::dml::{Column, Value, ValueKind, restore_inserts};
use platypus_sql::generated::{Generated, generated_columns};
use platypus_sql::snapshot::touched_rows;

use crate::import::{ImportError, rows_text};

/// The rows a change touched, as the statements that restore them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// `UPDATE` or `DELETE`.
    pub verb: &'static str,
    /// The table, qualified as the statements write it.
    pub table: String,
    pub rows: u64,
    pub restore: Vec<String>,
}

/// What a snapshot of a statement came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taken {
    Snapshot(Snapshot),
    /// Not a change a `SELECT` of one table tells the rows of (a join, a `LIMIT`…).
    Unsupported,
    /// It changes more rows than a snapshot keeps.
    TooMany,
    /// An `UPDATE` of a table without a primary key, or one that changes the key: its rows
    /// could not be found again.
    NoKey,
}

/// Reads the rows `sql` is about to change, up to `limit` of them, on `conn`.
///
/// # Errors
/// When the table cannot be described or its rows read.
pub async fn take(
    conn: &dyn Connection,
    dialect: Dialect,
    sql: &str,
    limit: usize,
) -> Result<Taken, ImportError> {
    let Some(touched) = touched_rows(sql, dialect) else {
        return Ok(Taken::Unsupported);
    };
    let schema = match &touched.table.schema {
        Some(schema) => schema.clone(),
        None => match conn.schemas().await?.into_iter().find(|s| s.current) {
            Some(s) => s.name,
            None => return Ok(Taken::Unsupported),
        },
    };
    let name = &touched.table.name;
    let details = conn.table_details(&schema, name).await?;
    let generated = generated_columns(conn, dialect, &schema, name)
        .await
        .unwrap_or_default();
    let how = |column: &str| {
        generated
            .iter()
            .find(|g| g.name == column)
            .map(|g| g.generated)
    };
    // A computed column comes back by itself.
    let columns: Vec<_> = details
        .columns
        .iter()
        .filter(|c| how(&c.name) != Some(Generated::Computed))
        .collect();
    let key: Vec<usize> = (0..columns.len())
        .filter(|&i| columns[i].primary_key)
        .collect();
    let sets_key = key.iter().any(|&i| {
        let name = &columns[i].name;
        touched
            .assigned
            .iter()
            .any(|a| a.eq_ignore_ascii_case(name))
    });
    if touched.verb == "UPDATE" && (key.is_empty() || sets_key) {
        return Ok(Taken::NoKey);
    }
    let quoted: Vec<String> = columns
        .iter()
        .map(|c| dialect.quote_ident(&c.name).into_owned())
        .collect();
    // PostgreSQL writes every value as its own text, which it reads back as the column's type.
    let list: Vec<String> = if dialect == Dialect::Postgres {
        quoted.iter().map(|q| format!("{q}::text")).collect()
    } else {
        quoted.clone()
    };
    let Some((kinds, rows)) = read(conn, &touched.select(&list.join(", ")), dialect, limit).await?
    else {
        return Ok(Taken::TooMany);
    };
    let table = dialect.qualified(Some(&schema), name);
    let described: Vec<Column<'_>> = columns
        .iter()
        .zip(
            kinds
                .iter()
                .copied()
                .chain(std::iter::repeat(ValueKind::Text)),
        )
        .map(|(c, kind)| Column {
            name: &c.name,
            kind,
            default: None,
        })
        .collect();
    let identity: Vec<usize> = (0..columns.len())
        .filter(|&i| how(&columns[i].name) == Some(Generated::Identity))
        .collect();
    let restore = if touched.verb == "DELETE" {
        let values: Vec<Vec<(usize, Value<'_>)>> = rows
            .iter()
            .map(|row| {
                row.iter()
                    .enumerate()
                    .map(|(i, v)| (i, v.as_deref().map_or(Value::Null, Value::Text)))
                    .collect()
            })
            .collect();
        restore_inserts(dialect, &table, &described, &values, &identity)?
    } else {
        updates(
            dialect,
            &table,
            (&described, &quoted),
            (&key, &identity),
            &rows,
        )?
    };
    Ok(Taken::Snapshot(Snapshot {
        verb: touched.verb,
        table,
        rows: rows.len() as u64,
        restore,
    }))
}

/// The rows of `select` as text, with the kind of each column; `None` past `limit` rows.
async fn read(
    conn: &dyn Connection,
    select: &str,
    dialect: Dialect,
    limit: usize,
) -> Result<Option<(Vec<ValueKind>, Vec<Vec<Option<String>>>)>, ImportError> {
    // Paged, so that a change of more rows than kept stops the read at once.
    let options = ExecOptions {
        batch_size: 1000,
        paged: true,
    };
    let ExecResult::Rows(mut stream) = conn.execute(select, &options).await? else {
        return Ok(Some((Vec::new(), Vec::new())));
    };
    let mut kinds = Vec::new();
    let mut rows = Vec::new();
    while let Some(batch) = stream.next().await {
        let batch = batch?;
        if kinds.is_empty() {
            kinds = batch
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
                .collect();
        }
        rows.extend(rows_text(&batch)?);
        if rows.len() > limit {
            return Ok(None);
        }
    }
    Ok(Some((kinds, rows)))
}

/// An `UPDATE` a row that sets its columns back, found by its key; identity columns, which
/// cannot be set, stay.
fn updates(
    dialect: Dialect,
    table: &str,
    (columns, quoted): (&[Column<'_>], &[String]),
    (key, identity): (&[usize], &[usize]),
    rows: &[Vec<Option<String>>],
) -> Result<Vec<String>, ImportError> {
    let set: Vec<usize> = (0..columns.len())
        .filter(|i| !key.contains(i) && !identity.contains(i))
        .collect();
    if set.is_empty() {
        return Ok(Vec::new());
    }
    // `column = value` for each of `at`, with the row's values.
    let equals = |row: &[Option<String>], at: &[usize]| {
        at.iter()
            .map(|&i| {
                let value = columns[i].literal(dialect, row.get(i).and_then(Option::as_deref))?;
                Ok(format!("{} = {value}", quoted[i]))
            })
            .collect::<Result<Vec<_>, ImportError>>()
    };
    rows.iter()
        .map(|row| {
            Ok(format!(
                "UPDATE {table} SET {} WHERE {}",
                equals(row, &set)?.join(", "),
                equals(row, key)?.join(" AND ")
            ))
        })
        .collect()
}
