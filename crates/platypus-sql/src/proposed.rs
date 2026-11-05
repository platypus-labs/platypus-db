//! Rows an agent proposes to change (`propose_edits`): the structure the proposal is kept in,
//! its checks against the table's real keys, and the statement that reads the rows to change.
//!
//! A proposal is kept in a file any program can write, so the app trusts none of it: it keeps
//! only structured data (names and values) and writes every statement itself from the table's
//! catalog, with the names quoted by the dialect and the values as literals of their
//! column's type. [`validate`] runs again where the proposal is opened.

use std::collections::HashSet;
use std::fmt::Write as _;

use platypus_core::TableDetails;
use serde::{Deserialize, Serialize};

use crate::Dialect;
use crate::dml::{Column, ValueKind};

/// Rows (inserted, updated and deleted together) one proposal holds at most.
pub const MAX_ROWS: usize = 500;

/// Bytes of a proposal, as [`Edits`] serializes, at most.
pub const MAX_BYTES: usize = 1024 * 1024;

/// A row's cells: column and value as text, `None` for NULL.
pub type Row = Vec<(String, Option<String>)>;

/// A row to change: found by its `key`, with the cells `set` to their new values.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Update {
    pub key: Row,
    pub set: Row,
}

/// The rows proposed for one table.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Edits {
    pub schema: String,
    pub table: String,
    #[serde(default)]
    pub inserts: Vec<Row>,
    #[serde(default)]
    pub updates: Vec<Update>,
    /// The key of each row to delete.
    #[serde(default)]
    pub deletes: Vec<Row>,
}

impl Edits {
    /// Rows of all three kinds.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.inserts.len() + self.updates.len() + self.deletes.len()
    }
}

/// The keys that tell the rows of a table apart: its primary key, then its unique
/// constraints and unique indexes over plain columns (not partial ones, nor expressions).
#[must_use]
pub fn unique_keys(details: &TableDetails) -> Vec<Vec<String>> {
    use platypus_core::ConstraintKind;
    let has = |name: &String| details.columns.iter().any(|c| &c.name == name);
    let mut keys: Vec<Vec<String>> = Vec::new();
    let primary: Vec<String> = details
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.clone())
        .collect();
    let declared = details
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::PrimaryKey | ConstraintKind::Unique))
        .map(|c| c.columns.clone());
    let indexed = details
        .indexes
        .iter()
        .filter(|i| (i.unique || i.primary) && !i.partial)
        .map(|i| i.columns.clone());
    for key in std::iter::once(primary).chain(declared).chain(indexed) {
        if key.is_empty() || !key.iter().all(has) {
            continue;
        }
        let same =
            |other: &Vec<String>| other.len() == key.len() && key.iter().all(|c| other.contains(c));
        if !keys.iter().any(same) {
            keys.push(key);
        }
    }
    keys
}

/// The key of `keys` that `row` names exactly, as a position.
fn key_named(row: &Row, keys: &[Vec<String>]) -> Option<usize> {
    let names: HashSet<&str> = row.iter().map(|(c, _)| c.as_str()).collect();
    keys.iter().position(|k| {
        k.len() == names.len()
            && k.len() == row.len()
            && k.iter().all(|c| names.contains(c.as_str()))
    })
}

fn describe_keys(keys: &[Vec<String>]) -> String {
    keys.iter()
        .map(|k| format!("({})", k.join(", ")))
        .collect::<Vec<_>>()
        .join(" or ")
}

/// A row's cells in one order, for telling duplicates.
fn signature(row: &Row) -> String {
    let mut cells: Vec<String> = row
        .iter()
        .map(|(c, v)| format!("{c}\u{0}{}", v.as_deref().unwrap_or("\u{1}")))
        .collect();
    cells.sort();
    cells.join("\u{2}")
}

/// What `what` ("a key", "values to set") says is wrong with the columns of `row`, if
/// anything: one that the table lacks, or one named twice.
fn columns_problem(row: &Row, details: &TableDetails, what: &str) -> Option<String> {
    let mut seen = HashSet::new();
    for (name, _) in row {
        if !details.columns.iter().any(|c| &c.name == name) {
            let known: Vec<&str> = details.columns.iter().map(|c| c.name.as_str()).collect();
            return Some(format!(
                "{what} names the column `{name}`, which the table does not have (it has {})",
                known.join(", ")
            ));
        }
        if !seen.insert(name) {
            return Some(format!("{what} names the column `{name}` twice"));
        }
    }
    None
}

/// Checks a row that finds rows by key (`key` of an update, an entry of `deletes`): exactly
/// the columns of one key, none NULL. Returns the key's position in `keys`.
fn check_key(
    row: &Row,
    keys: &[Vec<String>],
    details: &TableDetails,
    what: &str,
) -> Result<usize, String> {
    if let Some(problem) = columns_problem(row, details, what) {
        return Err(problem);
    }
    let Some(position) = key_named(row, keys) else {
        let given: Vec<&str> = row.iter().map(|(c, _)| c.as_str()).collect();
        return Err(format!(
            "{what} names ({}), which is not exactly the columns of a key of the table: its keys are {}",
            given.join(", "),
            describe_keys(keys)
        ));
    };
    if let Some((column, _)) = row.iter().find(|(_, v)| v.is_none()) {
        return Err(format!(
            "{what} has NULL for the key column `{column}`: a NULL matches no row"
        ));
    }
    Ok(position)
}

/// Checks `edits` against the table's catalog: the table and columns exist, the table has a
/// key, every row to update or delete is named by the columns of exactly one key (none NULL,
/// none twice), nothing is empty.
///
/// # Errors
/// A sentence saying what is wrong, for the agent (or the user) to correct.
pub fn validate(edits: &Edits, details: &TableDetails) -> Result<(), String> {
    let name = format!("{}.{}", edits.schema, edits.table);
    if details.columns.is_empty() {
        return Err(format!("`{name}` has no columns or does not exist"));
    }
    if edits.rows() == 0 {
        return Err("there are no changes to propose".to_owned());
    }
    let keys = unique_keys(details);
    if keys.is_empty() {
        return Err(format!(
            "`{name}` has no primary key or unique key, so its rows cannot be told apart: \
             propose SQL with propose_sql instead"
        ));
    }
    for (i, row) in edits.inserts.iter().enumerate() {
        let what = format!("inserts[{i}]");
        if row.is_empty() {
            return Err(format!("{what} has no columns"));
        }
        if let Some(problem) = columns_problem(row, details, &what) {
            return Err(problem);
        }
    }
    let mut seen = HashSet::new();
    for (i, update) in edits.updates.iter().enumerate() {
        check_key(&update.key, &keys, details, &format!("updates[{i}].key"))?;
        if update.set.is_empty() {
            return Err(format!(
                "updates[{i}].set is empty: there is nothing to change"
            ));
        }
        if let Some(problem) = columns_problem(&update.set, details, &format!("updates[{i}].set")) {
            return Err(problem);
        }
        if !seen.insert(signature(&update.key)) {
            return Err(format!(
                "updates[{i}] changes a row that another update changes"
            ));
        }
    }
    for (i, key) in edits.deletes.iter().enumerate() {
        check_key(key, &keys, details, &format!("deletes[{i}]"))?;
        if !seen.insert(signature(key)) {
            return Err(format!(
                "deletes[{i}] deletes a row that is also updated or deleted by another entry"
            ));
        }
    }
    Ok(())
}

/// How values of a column declared `data_type` are written in a condition. The declared type
/// is all there is before any row is read: numbers and booleans go bare, bytes in hex, the
/// rest as string literals the server converts.
#[must_use]
pub fn value_kind(data_type: &str) -> ValueKind {
    let lower = data_type.trim().to_ascii_lowercase();
    let word: String = lower
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let digits = |rest: &str| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit());
    let integer = matches!(
        word.as_str(),
        "int"
            | "integer"
            | "bigint"
            | "smallint"
            | "tinyint"
            | "mediumint"
            | "serial"
            | "bigserial"
            | "smallserial"
    ) || word.strip_prefix("int").is_some_and(digits)
        || word.strip_prefix("uint").is_some_and(digits)
        || word.strip_prefix("serial").is_some_and(digits);
    match word.as_str() {
        _ if integer => ValueKind::Integer,
        "bool" | "boolean" => ValueKind::Bool,
        "numeric" | "decimal" | "number" | "dec" | "money" | "fixed" => ValueKind::Decimal,
        "real" | "float4" | "float32" => ValueKind::Real,
        "float" | "double" | "float8" | "float64" | "binary_float" | "binary_double" => {
            ValueKind::Float
        }
        "bytea" | "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary" | "varbinary"
        | "raw" | "bytes" => ValueKind::Binary,
        "date" => ValueKind::Date,
        "timestamp" | "timestamptz" | "datetime" | "datetime2" | "smalldatetime"
        | "datetimeoffset" => ValueKind::Timestamp {
            zoned: lower.contains("with time zone")
                || lower.contains("timestamptz")
                || lower.contains("datetimeoffset"),
        },
        "time" | "timetz" => ValueKind::Time,
        _ => ValueKind::Text,
    }
}

/// What the data viewer opens a proposal with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    /// The viewer's statement: `SELECT * FROM t WHERE (k = v AND …) OR …` for the rows the
    /// updates and deletes name, the whole table (`SELECT * FROM t`) when there are only
    /// inserts.
    pub sql: String,
    /// The table, qualified and quoted.
    pub table: String,
}

/// The viewer's statement for `edits`, written from the catalog alone: every name is quoted
/// by `dialect`, every value is a literal of its column's type, and nothing of the proposal
/// is spliced in as SQL.
///
/// # Errors
/// When `edits` do not [`validate`] against `details`, or a key value is not one its column
/// takes (letters in a number column).
pub fn viewer(dialect: Dialect, edits: &Edits, details: &TableDetails) -> Result<Viewer, String> {
    validate(edits, details)?;
    let keys: Vec<&Row> = edits
        .updates
        .iter()
        .map(|u| &u.key)
        .chain(&edits.deletes)
        .collect();
    let table = dialect.qualified(Some(&edits.schema), &edits.table);
    // A column `SELECT *` leaves out that rows are edited by (CockroachDB's `rowid`).
    let list = details.columns.iter().find(|c| c.hidden).map_or_else(
        || "*".to_owned(),
        |c| format!("*, {}", dialect.quote_ident(&c.name)),
    );
    let mut sql = format!("SELECT {list}\nFROM {table}");
    if !keys.is_empty() {
        let _ = write!(sql, "\nWHERE {}", key_condition(dialect, &keys, details)?);
    }
    Ok(Viewer { sql, table })
}

/// The condition matching the rows `keys` name: `(k = v AND …) OR …`, the names quoted by
/// `dialect` and every value a literal of its column's type.
fn key_condition(
    dialect: Dialect,
    keys: &[&Row],
    details: &TableDetails,
) -> Result<String, String> {
    let mut condition = String::new();
    for (n, key) in keys.iter().enumerate() {
        if n > 0 {
            condition.push_str(" OR ");
        }
        condition.push('(');
        for (i, (name, value)) in key.iter().enumerate() {
            let declared = details
                .columns
                .iter()
                .find(|c| &c.name == name)
                .map_or("", |c| c.data_type.as_str());
            let column = Column {
                name,
                kind: value_kind(declared),
                default: None,
            };
            if i > 0 {
                condition.push_str(" AND ");
            }
            let literal = column
                .literal(dialect, value.as_deref())
                .map_err(|e| e.to_string())?;
            let _ = write!(condition, "{} = {literal}", dialect.quote_ident(name));
        }
        condition.push(')');
    }
    Ok(condition)
}

#[cfg(test)]
mod tests {
    use platypus_core::{ColumnMeta, ConstraintKind, ConstraintMeta, IndexMeta};

    use super::*;

    fn column(name: &str, data_type: &str, primary_key: bool) -> ColumnMeta {
        ColumnMeta {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable: !primary_key,
            default: None,
            primary_key,
            comment: None,
            enum_values: Vec::new(),
            fields: Vec::new(),
            hidden: false,
        }
    }

    fn table() -> TableDetails {
        TableDetails {
            columns: vec![
                column("id", "integer", true),
                column("name", "text", false),
                column("email", "text", false),
            ],
            constraints: vec![ConstraintMeta {
                name: "u".to_owned(),
                kind: ConstraintKind::Unique,
                columns: vec!["email".to_owned()],
                definition: String::new(),
            }],
            ..TableDetails::default()
        }
    }

    fn row(cells: &[(&str, Option<&str>)]) -> Row {
        cells
            .iter()
            .map(|(c, v)| ((*c).to_owned(), v.map(str::to_owned)))
            .collect()
    }

    fn edits() -> Edits {
        Edits {
            schema: "main".to_owned(),
            table: "t".to_owned(),
            ..Edits::default()
        }
    }

    #[test]
    fn keys_are_the_primary_key_then_the_unique_ones() {
        let mut details = table();
        details.indexes.push(IndexMeta {
            name: "i".to_owned(),
            columns: vec!["name".to_owned(), "lower(email)".to_owned()],
            unique: true,
            primary: false,
            partial: false,
            detail: None,
            size: None,
        });
        details.indexes.push(IndexMeta {
            name: "email_again".to_owned(),
            columns: vec!["email".to_owned()],
            unique: true,
            primary: false,
            partial: false,
            detail: None,
            size: None,
        });
        assert_eq!(
            unique_keys(&details),
            vec![vec!["id".to_owned()], vec!["email".to_owned()]]
        );
    }

    #[test]
    fn refuses_what_is_wrong_with_a_proposal() {
        let details = table();
        let mut e = edits();
        assert!(validate(&e, &details).unwrap_err().contains("no changes"));
        e.inserts.push(row(&[("nope", Some("1"))]));
        assert!(validate(&e, &details).unwrap_err().contains("`nope`"));
        e.inserts = vec![row(&[("name", Some("a")), ("name", Some("b"))])];
        assert!(validate(&e, &details).unwrap_err().contains("twice"));
        e.inserts.clear();
        e.deletes.push(row(&[("name", Some("a"))]));
        assert!(validate(&e, &details).unwrap_err().contains("not exactly"));
        e.deletes = vec![row(&[("id", Some("1")), ("name", Some("a"))])];
        assert!(validate(&e, &details).unwrap_err().contains("not exactly"));
        e.deletes = vec![row(&[("id", None)])];
        assert!(validate(&e, &details).unwrap_err().contains("NULL"));
        e.deletes = vec![row(&[("id", Some("1"))]), row(&[("id", Some("1"))])];
        assert!(validate(&e, &details).unwrap_err().contains("also updated"));
        e.deletes.clear();
        e.updates.push(Update {
            key: row(&[("id", Some("1"))]),
            set: Vec::new(),
        });
        assert!(
            validate(&e, &details)
                .unwrap_err()
                .contains("nothing to change")
        );
        e.updates[0].set = row(&[("name", Some("x"))]);
        assert_eq!(validate(&e, &details), Ok(()));
        // A key of the unique constraint is as good as the primary key.
        e.updates[0].key = row(&[("email", Some("a@b.c"))]);
        assert_eq!(validate(&e, &details), Ok(()));
        let mut keyless = table();
        keyless.columns[0].primary_key = false;
        keyless.constraints.clear();
        assert!(
            validate(&e, &keyless)
                .unwrap_err()
                .contains("no primary key")
        );
        assert!(validate(&e, &TableDetails::default()).is_err());
    }

    #[test]
    fn the_viewer_statement_quotes_names_and_values() {
        let details = table();
        let mut e = edits();
        e.inserts.push(row(&[("name", Some("new"))]));
        assert_eq!(
            viewer(Dialect::Sqlite, &e, &details).unwrap().sql,
            "SELECT *\nFROM main.t"
        );
        e.updates.push(Update {
            key: row(&[("id", Some("7"))]),
            set: row(&[("name", Some("x"))]),
        });
        e.deletes
            .push(row(&[("email", Some("o'brien; DROP TABLE t; --"))]));
        let sql = viewer(Dialect::Sqlite, &e, &details).unwrap().sql;
        assert_eq!(
            sql,
            "SELECT *\nFROM main.t\nWHERE (id = 7) OR (email = 'o''brien; DROP TABLE t; --')"
        );
        // A number column takes a number only.
        e.updates[0].key = row(&[("id", Some("1 OR 1=1"))]);
        assert!(viewer(Dialect::Sqlite, &e, &details).is_err());
        // Hostile names stay inside their quotes.
        let mut odd = TableDetails {
            columns: vec![column("a\"; DROP TABLE x; --", "text", true)],
            ..TableDetails::default()
        };
        odd.columns[0].primary_key = true;
        let mut e = Edits {
            schema: "s\"x".to_owned(),
            table: "t\"; DROP".to_owned(),
            ..Edits::default()
        };
        e.deletes.push(row(&[("a\"; DROP TABLE x; --", Some("v"))]));
        let sql = viewer(Dialect::Postgres, &e, &odd).unwrap().sql;
        assert_eq!(
            sql,
            "SELECT *\nFROM \"s\"\"x\".\"t\"\"; DROP\"\nWHERE (\"a\"\"; DROP TABLE x; --\" = 'v')"
        );
    }

    #[test]
    fn composite_keys_and_hidden_columns() {
        let mut details = TableDetails {
            columns: vec![
                column("a", "int", true),
                column("b", "varchar(20)", true),
                column("rowid", "int", false),
            ],
            ..TableDetails::default()
        };
        details.columns[2].hidden = true;
        let mut e = edits();
        e.deletes.push(row(&[("b", Some("x")), ("a", Some("1"))]));
        let sql = viewer(Dialect::MySql, &e, &details).unwrap().sql;
        assert!(sql.starts_with("SELECT *, rowid\nFROM"), "{sql}");
        assert!(sql.ends_with("WHERE (b = 'x' AND a = 1)"), "{sql}");
        e.deletes[0] = row(&[("a", Some("1"))]);
        assert!(viewer(Dialect::MySql, &e, &details).is_err());
    }

    #[test]
    fn value_kinds_from_declared_types() {
        assert_eq!(value_kind("INTEGER"), ValueKind::Integer);
        assert_eq!(value_kind("bigint unsigned"), ValueKind::Integer);
        assert_eq!(value_kind("int4"), ValueKind::Integer);
        assert_eq!(value_kind("interval"), ValueKind::Text);
        assert_eq!(value_kind("point"), ValueKind::Text);
        assert_eq!(value_kind("numeric(12,2)"), ValueKind::Decimal);
        assert_eq!(value_kind("boolean"), ValueKind::Bool);
        assert_eq!(value_kind("bytea"), ValueKind::Binary);
        assert_eq!(value_kind("character varying(10)"), ValueKind::Text);
        assert_eq!(
            value_kind("timestamp with time zone"),
            ValueKind::Timestamp { zoned: true }
        );
    }
}
