//! The `UPDATE`, `INSERT` and `DELETE` statements that save the data editor's changes.
//!
//! Values arrive as text, the way the grid shows them, and are written as literals of the
//! column's type: numbers bare, booleans and binary values in the dialect's syntax, the rest
//! as string literals that the server converts to the column type. Rows are identified by a
//! key (primary key or non-null unique key); every statement must hit exactly one row, which
//! [`apply`] checks while running them all or none.

use std::fmt;

use platypus_core::arrow_schema::DataType;
use platypus_core::{Connection, Error, ExecOptions, ExecResult};

use crate::Dialect;

/// How a column's values are written in SQL, from its Arrow type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Bool,
    Integer,
    /// Exact numbers (decimals).
    Decimal,
    Float,
    /// Hex text, e.g. `deadbeef`.
    Binary,
    /// `zoned` timestamps are shown with their UTC offset.
    Timestamp {
        zoned: bool,
    },
    Text,
}

impl ValueKind {
    #[must_use]
    pub fn of(data_type: &DataType) -> Self {
        match data_type {
            DataType::Boolean => Self::Bool,
            t if t.is_integer() => Self::Integer,
            t if t.is_floating() => Self::Float,
            t if t.is_numeric() => Self::Decimal,
            DataType::Binary
            | DataType::LargeBinary
            | DataType::BinaryView
            | DataType::FixedSizeBinary(_) => Self::Binary,
            DataType::Timestamp(_, tz) => Self::Timestamp {
                zoned: tz.is_some(),
            },
            _ => Self::Text,
        }
    }
}

/// A value to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value<'a> {
    /// Text as the grid shows it, converted to the column's type.
    Text(&'a str),
    Null,
    /// The column default.
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column<'a> {
    pub name: &'a str,
    pub kind: ValueKind,
    /// The default expression, written where the dialect has no `DEFAULT` keyword.
    pub default: Option<&'a str>,
}

/// A value the column cannot take, e.g. letters in a number column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidValue {
    pub column: String,
    pub message: String,
}

impl fmt::Display for InvalidValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.column, self.message)
    }
}

impl std::error::Error for InvalidValue {}

pub type Result<T, E = InvalidValue> = std::result::Result<T, E>;

/// `[+-]digits[.digits][e[+-]digits]`, or with digits only after the point.
fn is_number(s: &str) -> bool {
    let s = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (mantissa, exponent) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |p: &str| p.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = (!int.is_empty() || !frac.is_empty()) && digits(int) && digits(frac);
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mantissa_ok && exponent_ok
}

fn is_integer(s: &str) -> bool {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

fn parse_bool(s: &str) -> Option<bool> {
    const TRUE: &[&str] = &["true", "t", "1", "yes", "y", "on"];
    const FALSE: &[&str] = &["false", "f", "0", "no", "n", "off"];
    if TRUE.iter().any(|w| w.eq_ignore_ascii_case(s)) {
        Some(true)
    } else if FALSE.iter().any(|w| w.eq_ignore_ascii_case(s)) {
        Some(false)
    } else {
        None
    }
}

/// Hex digits, with an optional `\x` or `0x` prefix, lower-cased.
fn parse_hex(s: &str) -> Option<String> {
    let hex = s
        .strip_prefix("\\x")
        .or_else(|| s.strip_prefix("0x"))
        .unwrap_or(s);
    (hex.len().is_multiple_of(2) && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

impl Column<'_> {
    fn invalid(&self, message: impl Into<String>) -> InvalidValue {
        InvalidValue {
            column: self.name.to_owned(),
            message: message.into(),
        }
    }

    /// `text` as a literal of the column's type; `None` is NULL.
    pub fn literal(&self, dialect: Dialect, text: Option<&str>) -> Result<String> {
        let Some(text) = text else {
            return Ok("NULL".to_owned());
        };
        let trimmed = text.trim();
        Ok(match self.kind {
            ValueKind::Integer if is_integer(trimmed) => trimmed.to_owned(),
            ValueKind::Integer => {
                return Err(self.invalid(format!("'{text}' is not a whole number")));
            }
            ValueKind::Decimal | ValueKind::Float if is_number(trimmed) => trimmed.to_owned(),
            ValueKind::Float
                if ["nan", "infinity", "+infinity", "-infinity", "inf", "-inf"]
                    .iter()
                    .any(|w| w.eq_ignore_ascii_case(trimmed)) =>
            {
                dialect.quote_literal(trimmed)
            }
            ValueKind::Decimal | ValueKind::Float => {
                return Err(self.invalid(format!("'{text}' is not a number")));
            }
            ValueKind::Bool => match parse_bool(trimmed) {
                Some(b) => dialect.bool_literal(b).to_owned(),
                None => return Err(self.invalid(format!("'{text}' is not true or false"))),
            },
            ValueKind::Binary => match parse_hex(trimmed) {
                Some(hex) => dialect.binary_literal(&hex),
                None => {
                    return Err(
                        self.invalid(format!("'{text}' is not hexadecimal bytes (e.g. 0a1b)"))
                    );
                }
            },
            // MySQL sessions run in UTC and MariaDB rejects offsets in datetime literals.
            ValueKind::Timestamp { zoned: true } if dialect == Dialect::MySql => {
                let local = trimmed
                    .strip_suffix("+00:00")
                    .or_else(|| trimmed.strip_suffix('Z'))
                    .unwrap_or(trimmed);
                dialect.quote_literal(local)
            }
            ValueKind::Timestamp { .. } | ValueKind::Text => dialect.quote_literal(text),
        })
    }

    /// The right-hand side of `SET column = …`.
    fn assignment(&self, dialect: Dialect, value: Value<'_>) -> Result<String> {
        match value {
            Value::Text(text) => self.literal(dialect, Some(text)),
            Value::Null => self.literal(dialect, None),
            Value::Default if dialect.has_default_keyword() => Ok("DEFAULT".to_owned()),
            Value::Default => Ok(self.default.unwrap_or("NULL").to_owned()),
        }
    }
}

fn column<'c, 'a>(columns: &'c [Column<'a>], index: usize) -> Result<&'c Column<'a>> {
    columns.get(index).ok_or_else(|| InvalidValue {
        column: format!("#{index}"),
        message: "no such column".to_owned(),
    })
}

/// `WHERE k1 = … AND k2 IS NULL`; `key` pairs column indexes with the row's current values.
fn where_clause(
    dialect: Dialect,
    columns: &[Column<'_>],
    key: &[(usize, Option<&str>)],
) -> Result<String> {
    if key.is_empty() {
        return Err(InvalidValue {
            column: String::new(),
            message: "the row has no key to identify it".to_owned(),
        });
    }
    let mut out = String::from(" WHERE ");
    for (i, &(index, value)) in key.iter().enumerate() {
        let c = column(columns, index)?;
        if i > 0 {
            out.push_str(" AND ");
        }
        out.push_str(&dialect.quote_ident(c.name));
        if value.is_none() {
            out.push_str(" IS NULL");
        } else {
            out.push_str(" = ");
            out.push_str(&c.literal(dialect, value)?);
        }
    }
    Ok(out)
}

/// `UPDATE table SET … WHERE key`; `table` is quoted already, e.g. by [`Dialect::qualified`].
pub fn update(
    dialect: Dialect,
    table: &str,
    columns: &[Column<'_>],
    set: &[(usize, Value<'_>)],
    key: &[(usize, Option<&str>)],
) -> Result<String> {
    let mut sql = format!("UPDATE {table} SET ");
    for (i, &(index, value)) in set.iter().enumerate() {
        let c = column(columns, index)?;
        if i > 0 {
            sql.push_str(", ");
        }
        sql.push_str(&dialect.quote_ident(c.name));
        sql.push_str(" = ");
        sql.push_str(&c.assignment(dialect, value)?);
    }
    sql.push_str(&where_clause(dialect, columns, key)?);
    Ok(sql)
}

/// `INSERT INTO table (…) VALUES (…)`. Columns left at their default are omitted.
pub fn insert(
    dialect: Dialect,
    table: &str,
    columns: &[Column<'_>],
    values: &[(usize, Value<'_>)],
) -> Result<String> {
    let mut names = Vec::with_capacity(values.len());
    let mut literals = Vec::with_capacity(values.len());
    for &(index, value) in values {
        let c = column(columns, index)?;
        let text = match value {
            Value::Default => continue,
            Value::Null => None,
            Value::Text(text) => Some(text),
        };
        names.push(dialect.quote_ident(c.name));
        literals.push(c.literal(dialect, text)?);
    }
    if names.is_empty() {
        return Ok(dialect.insert_defaults(table));
    }
    Ok(format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        names.join(", "),
        literals.join(", ")
    ))
}

/// `DELETE FROM table WHERE key`.
pub fn delete(
    dialect: Dialect,
    table: &str,
    columns: &[Column<'_>],
    key: &[(usize, Option<&str>)],
) -> Result<String> {
    Ok(format!(
        "DELETE FROM {table}{}",
        where_clause(dialect, columns, key)?
    ))
}

/// Why [`apply`] rolled back.
#[derive(Debug)]
pub enum ApplyError {
    /// A statement failed; `index` is its position in the list.
    Failed { index: usize, error: Error },
    /// A statement hit `rows` rows instead of one: the row changed or disappeared since it
    /// was loaded, or the key is not unique after all.
    RowCount { index: usize, rows: u64 },
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed { index, error } => write!(f, "statement {}: {error}", index + 1),
            Self::RowCount { index, rows: 0 } => write!(
                f,
                "statement {}: the row was not found; it may have been changed or deleted since it was loaded",
                index + 1
            ),
            Self::RowCount { index, rows } => write!(
                f,
                "statement {} would change {rows} rows instead of one",
                index + 1
            ),
        }
    }
}

impl std::error::Error for ApplyError {}

async fn exec_one(conn: &dyn Connection, sql: &str) -> Result<u64, Error> {
    match conn.execute(sql, &ExecOptions::default()).await? {
        ExecResult::Affected { rows } => Ok(rows),
        // Dropping the stream ends the statement.
        ExecResult::Rows(_) => Ok(0),
    }
}

const SAVEPOINT: &str = "platypus_edit";

/// Runs `statements`, each of which must hit exactly one row, all or none. In autocommit
/// mode they run in a transaction of their own and are committed; in manual-commit mode
/// they join the session's transaction behind a savepoint, and stay uncommitted. Without
/// savepoints (DuckDB) a failed statement in manual-commit mode rolls back the whole
/// transaction, which DuckDB aborts on errors anyway; so does a row count off once rows
/// changed, as nothing else can undo them. A first statement that finds no row leaves the
/// transaction as it was.
pub async fn apply(
    conn: &dyn Connection,
    dialect: Dialect,
    statements: &[String],
) -> Result<(), ApplyError> {
    let manual = !conn.transaction().autocommit;
    let savepointless = manual && !dialect.has_savepoints();
    let (begin, end, undo) = if savepointless {
        (None, Vec::new(), vec!["ROLLBACK".to_owned()])
    } else if manual {
        (
            Some(format!("SAVEPOINT {SAVEPOINT}")),
            vec![format!("RELEASE SAVEPOINT {SAVEPOINT}")],
            vec![
                format!("ROLLBACK TO SAVEPOINT {SAVEPOINT}"),
                format!("RELEASE SAVEPOINT {SAVEPOINT}"),
            ],
        )
    } else {
        (
            Some("BEGIN".to_owned()),
            vec!["COMMIT".to_owned()],
            vec!["ROLLBACK".to_owned()],
        )
    };
    if let Some(begin) = begin {
        exec_one(conn, &begin)
            .await
            .map_err(|error| ApplyError::Failed { index: 0, error })?;
    }
    let mut outcome = Ok(());
    for (index, sql) in statements.iter().enumerate() {
        match exec_one(conn, sql).await {
            Ok(1) => {}
            Ok(rows) => outcome = Err(ApplyError::RowCount { index, rows }),
            Err(error) => outcome = Err(ApplyError::Failed { index, error }),
        }
        if outcome.is_err() {
            break;
        }
    }
    let untouched = matches!(outcome, Err(ApplyError::RowCount { index: 0, rows: 0 }));
    let finish = match outcome {
        Ok(()) => end,
        Err(_) if savepointless && untouched => Vec::new(),
        Err(_) => undo,
    };
    for sql in &finish {
        if let Err(error) = exec_one(conn, sql).await {
            // The first error explains more than the failed cleanup.
            return outcome.and(Err(ApplyError::Failed {
                index: statements.len(),
                error,
            }));
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn col(name: &str, kind: ValueKind) -> Column<'_> {
        Column {
            name,
            kind,
            default: None,
        }
    }

    fn table(dialect: Dialect) -> String {
        dialect.qualified(Some("demo"), "Items")
    }

    fn columns() -> Vec<Column<'static>> {
        vec![
            col("id", ValueKind::Integer),
            col("name", ValueKind::Text),
            col("price", ValueKind::Decimal),
            col("active", ValueKind::Bool),
            col("data", ValueKind::Binary),
            col("seen", ValueKind::Timestamp { zoned: true }),
            Column {
                name: "order",
                kind: ValueKind::Integer,
                default: Some("0"),
            },
        ]
    }

    #[test]
    fn checks_numbers() {
        for ok in ["1", "-1", "+2.5", ".5", "5.", "1e10", "1.5E-3"] {
            assert!(is_number(ok), "{ok}");
        }
        for bad in ["", "-", ".", "1e", "1.2.3", "abc", "1 2", "0x10"] {
            assert!(!is_number(bad), "{bad}");
        }
        assert!(is_integer("-42"));
        assert!(!is_integer("4.2"));
        assert!(!is_integer("+"));
    }

    #[test]
    fn updates_with_typed_literals() {
        let cols = columns();
        let set = [
            (1, Value::Text("it's")),
            (2, Value::Text(" 12.50 ")),
            (3, Value::Text("yes")),
            (4, Value::Text("\\xDEAD")),
            (6, Value::Default),
        ];
        let key = [(0, Some("7"))];
        assert_eq!(
            update(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &set,
                &key
            )
            .unwrap(),
            "UPDATE demo.\"Items\" SET name = 'it''s', price = 12.50, active = TRUE, \
             data = decode('dead', 'hex'), \"order\" = DEFAULT WHERE id = 7"
        );
        assert_eq!(
            update(Dialect::Sqlite, &table(Dialect::Sqlite), &cols, &set, &key).unwrap(),
            "UPDATE demo.Items SET name = 'it''s', price = 12.50, active = 1, \
             data = X'dead', \"order\" = 0 WHERE id = 7"
        );
        assert_eq!(
            update(
                Dialect::MySql,
                &table(Dialect::MySql),
                &cols,
                &[(1, Value::Null)],
                &[(0, Some("1")), (1, None)]
            )
            .unwrap(),
            "UPDATE demo.Items SET name = NULL WHERE id = 1 AND name IS NULL"
        );
    }

    #[test]
    fn rejects_values_of_the_wrong_type() {
        let cols = columns();
        let err = update(
            Dialect::Postgres,
            &table(Dialect::Postgres),
            &cols,
            &[(0, Value::Text("x1"))],
            &[(0, Some("1"))],
        )
        .unwrap_err();
        assert_eq!(err.column, "id");
        assert!(err.message.contains("whole number"), "{err}");
        assert!(
            update(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &[(3, Value::Text("maybe"))],
                &[(0, Some("1"))]
            )
            .is_err()
        );
        assert!(
            update(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &[(4, Value::Text("abc"))],
                &[(0, Some("1"))]
            )
            .is_err()
        );
        assert!(
            update(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &[(1, Value::Text("x"))],
                &[]
            )
            .is_err(),
            "no key"
        );
        assert!(
            update(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &[(9, Value::Null)],
                &[(0, Some("1"))]
            )
            .is_err()
        );
    }

    #[test]
    fn writes_timestamps_for_each_dialect() {
        let cols = columns();
        let set = [(5, Value::Text("2024-03-01 10:00:00.5+00:00"))];
        let key = [(0, Some("1"))];
        assert!(
            update(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &set,
                &key
            )
            .unwrap()
            .contains("seen = '2024-03-01 10:00:00.5+00:00'")
        );
        assert!(
            update(Dialect::MySql, &table(Dialect::MySql), &cols, &set, &key)
                .unwrap()
                .contains("seen = '2024-03-01 10:00:00.5'")
        );
    }

    #[test]
    fn inserts_and_deletes() {
        let cols = columns();
        assert_eq!(
            insert(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &[(0, Value::Default), (1, Value::Text("a")), (2, Value::Null)]
            )
            .unwrap(),
            "INSERT INTO demo.\"Items\" (name, price) VALUES ('a', NULL)"
        );
        assert_eq!(
            insert(
                Dialect::Postgres,
                &table(Dialect::Postgres),
                &cols,
                &[(0, Value::Default)]
            )
            .unwrap(),
            "INSERT INTO demo.\"Items\" DEFAULT VALUES"
        );
        assert_eq!(
            insert(Dialect::MySql, "t", &cols, &[]).unwrap(),
            "INSERT INTO t () VALUES ()"
        );
        assert_eq!(
            delete(
                Dialect::MySql,
                &table(Dialect::MySql),
                &cols,
                &[(0, Some("3")), (1, Some("a`b"))]
            )
            .unwrap(),
            "DELETE FROM demo.Items WHERE id = 3 AND name = 'a`b'"
        );
    }
}
