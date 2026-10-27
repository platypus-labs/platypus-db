//! Copy of a table into a table of another connection, optionally created from the source
//! types: a data file's (read through a DuckDB connection on the file) or another
//! database's.
//!
//! Rows travel as multi-row `INSERT` statements with literals of the source column types, in
//! one transaction: a failed import leaves the target as it was, and a table it created is
//! dropped again (MySQL commits DDL on its own). Rows can also replace the table's or update
//! the rows with the same key.

use std::sync::Arc;

use arrow_array::{Array, RecordBatch};
use arrow_cast::display::FormatOptions;
use arrow_schema::{ArrowError, DataType, Field, Schema, TimeUnit};
use futures::StreamExt;
use platypus_core::{
    Connection, DriverKind, ExecOptions, ExecResult, META_DB_TYPE, RecordBatchStream, special,
};
use platypus_sql::Dialect;
use platypus_sql::dml::{Column, InvalidValue, ValueKind};
use serde::Deserialize;

/// Rows per `INSERT`: SQL Server takes at most 1000 in a `VALUES` list.
const ROWS_PER_INSERT: usize = 500;

/// Rows per statement of a bulk insert (see [`bulk`]).
const BULK_ROWS: usize = 5000;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ImportColumn {
    /// The column of the source table.
    pub source: String,
    /// The column of the target table.
    pub target: String,
    /// The column type, used when the import creates the table.
    #[serde(default)]
    pub data_type: Option<String>,
    /// A created column refuses NULL.
    #[serde(default)]
    pub not_null: bool,
    /// The default of a created column, as the target writes it.
    #[serde(default)]
    pub default: Option<String>,
}

/// What happens to the rows already in an existing table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    /// They stay; the new rows are added.
    #[default]
    Append,
    /// They are deleted first, in the same transaction (not `TRUNCATE`, which MySQL commits).
    Replace,
    /// A new row whose key exists updates that row instead.
    Upsert,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ImportPlan {
    /// The table or view the rows come from: in the file's DuckDB database, or in the source
    /// connection's `source_schema`.
    pub source_table: String,
    #[serde(default)]
    pub source_schema: Option<String>,
    pub schema: Option<String>,
    pub table: String,
    /// Create the table first, from the columns' `data_type`.
    pub create: bool,
    #[serde(default)]
    pub mode: ImportMode,
    /// The target columns that identify a row, for [`ImportMode::Upsert`].
    #[serde(default)]
    pub keys: Vec<String>,
    pub columns: Vec<ImportColumn>,
    /// The primary key of a created table, as target columns.
    #[serde(default)]
    pub primary_key: Vec<String>,
    /// The column of a created table that the database numbers (serial, auto-increment), the
    /// primary key; the imported rows keep their values.
    #[serde(skip)]
    pub identity: Option<String>,
    /// Constraints declared with a created table, e.g. `CONSTRAINT c CHECK (n > 0)`.
    #[serde(skip)]
    pub constraints: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error(transparent)]
    Database(#[from] platypus_core::Error),
    #[error(transparent)]
    Value(#[from] InvalidValue),
    #[error(transparent)]
    Arrow(#[from] ArrowError),
    #[error("{0}")]
    Plan(String),
    #[error("the import was cancelled")]
    Cancelled,
}

impl ImportPlan {
    fn table(&self, dialect: Dialect) -> String {
        dialect.qualified(self.schema.as_deref(), &self.table)
    }

    /// The mode that applies: a table the import creates has no rows to keep.
    fn mode(&self) -> ImportMode {
        if self.create {
            ImportMode::Append
        } else {
            self.mode
        }
    }

    fn check(&self) -> Result<(), ImportError> {
        if self.columns.is_empty() {
            return Err(ImportError::Plan("no column to import".to_owned()));
        }
        if self.mode() == ImportMode::Upsert {
            if self.keys.is_empty() {
                return Err(ImportError::Plan(
                    "an upsert needs the key columns".to_owned(),
                ));
            }
            if let Some(k) = self
                .keys
                .iter()
                .find(|k| !self.columns.iter().any(|c| &c.target == *k))
            {
                return Err(ImportError::Plan(format!(
                    "the key column {k} is not imported"
                )));
            }
        }
        Ok(())
    }

    /// `CREATE TABLE` with the columns of the plan.
    pub fn create_sql(&self, dialect: Dialect) -> Result<String, ImportError> {
        let mut columns = self
            .columns
            .iter()
            .map(|c| match c.data_type.as_deref().map(str::trim) {
                Some(t) if !t.is_empty() => Ok(format!(
                    "{} {}{}{}",
                    dialect.quote_ident(&c.target),
                    self.column_type(dialect, &c.target, t),
                    c.default
                        .as_ref()
                        .map_or_else(String::new, |d| default_clause(dialect, t, d)),
                    if c.not_null {
                        " NOT NULL"
                    } else if dialect == Dialect::ClickHouse
                        && !t.starts_with("Nullable(")
                        && !t.starts_with("LowCardinality(Nullable(")
                    {
                        // ClickHouse columns are not nullable unless declared so; `NULL`
                        // fails on types that already are.
                        " NULL"
                    } else {
                        ""
                    }
                )),
                _ => Err(ImportError::Plan(format!(
                    "column {} has no type",
                    c.target
                ))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !self.primary_key.is_empty() {
            let names: Vec<_> = self
                .primary_key
                .iter()
                .map(|k| dialect.quote_ident(k))
                .collect();
            columns.push(format!("PRIMARY KEY ({})", names.join(", ")));
        }
        columns.extend(self.constraints.iter().cloned());
        Ok(format!(
            "CREATE TABLE {} ({})",
            self.table(dialect),
            columns.join(", ")
        ))
    }

    /// The type of `column` in `CREATE TABLE`: `declared`, numbered by the database for the
    /// identity column (DuckDB gets a sequence once the rows are in).
    fn column_type(&self, dialect: Dialect, column: &str, declared: &str) -> String {
        if self.identity.as_deref() != Some(column) {
            return declared.to_owned();
        }
        match dialect {
            Dialect::Postgres => format!("{declared} GENERATED BY DEFAULT AS IDENTITY"),
            Dialect::MySql => format!("{declared} AUTO_INCREMENT"),
            Dialect::SqlServer => format!("{declared} IDENTITY(1, 1)"),
            // The rowid, which a BIGINT key is not.
            Dialect::Sqlite => "INTEGER".to_owned(),
            Dialect::DuckDb
            | Dialect::ClickHouse
            | Dialect::Ansi
            | Dialect::Snowflake
            | Dialect::BigQuery
            | Dialect::Databricks
            | Dialect::Oracle
            | Dialect::Db2 => declared.to_owned(),
        }
    }

    /// SQL Server's `SET IDENTITY_INSERT` of a created identity column, on or off: it takes
    /// the copied values only so.
    fn identity_insert(&self, dialect: Dialect, on: bool) -> Option<String> {
        (dialect == Dialect::SqlServer && self.create && self.identity.is_some()).then(|| {
            format!(
                "SET IDENTITY_INSERT {} {}",
                self.table(dialect),
                if on { "ON" } else { "OFF" }
            )
        })
    }

    /// The query of the source columns, in the source's dialect.
    #[must_use]
    pub fn source_sql(&self, dialect: Dialect) -> String {
        let columns: Vec<_> = self
            .columns
            .iter()
            .map(|c| dialect.quote_ident(&c.source))
            .collect();
        format!(
            "SELECT {} FROM {}",
            columns.join(", "),
            dialect.qualified(self.source_schema.as_deref(), &self.source_table)
        )
    }

    /// The `INSERT` (or upsert) statements of `batch`, whose columns are the plan's in order.
    pub fn insert_sql(
        &self,
        dialect: Dialect,
        batch: &RecordBatch,
    ) -> Result<Vec<String>, ImportError> {
        let columns = self.target_columns(&batch.schema());
        let names: Vec<&str> = columns.iter().map(|c| c.name).collect();
        let keys: Vec<&str> = self.keys.iter().map(String::as_str).collect();
        let table = self.table(dialect);
        let head = format!(
            "INSERT INTO {table} ({}) VALUES ",
            names
                .iter()
                .map(|c| dialect.quote_ident(c))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut rows = rows_text(batch)?;
        if self.mode() == ImportMode::Upsert {
            // The last row of a key wins, as later lines of a file are newer: PostgreSQL's
            // `ON CONFLICT` and SQL Server's `MERGE` change no row twice in a statement, and
            // the staged insert would add each.
            let at: Vec<usize> = keys
                .iter()
                .filter_map(|k| names.iter().position(|n| n == k))
                .collect();
            rows = last_per_key(rows, &at);
        }
        let mut statements = Vec::new();
        for chunk in rows.chunks(ROWS_PER_INSERT) {
            let values = chunk
                .iter()
                .map(|row| {
                    let literals = columns
                        .iter()
                        .zip(&self.columns)
                        .zip(row)
                        .map(|((c, plan), text)| {
                            let lit = literal(c, dialect, text.as_deref())?;
                            Ok(match (c.kind, text) {
                                (ValueKind::Text, Some(_)) => {
                                    typed_text(dialect, plan.data_type.as_deref(), lit)
                                }
                                _ => lit,
                            })
                        })
                        .collect::<Result<Vec<_>, InvalidValue>>()?;
                    Ok(format!("({})", literals.join(", ")))
                })
                .collect::<Result<Vec<_>, ImportError>>()?
                .join(", ");
            match (self.mode(), self.staging(dialect)) {
                (ImportMode::Upsert, Some(staging)) => {
                    statements.extend(self.staged_upsert(dialect, &staging, &names, &values));
                }
                (ImportMode::Upsert, None) => {
                    statements.push(
                        dialect
                            .upsert_rows(&table, &names, &keys, &values)
                            .ok_or_else(|| {
                                ImportError::Plan("ClickHouse has no upsert".to_owned())
                            })?,
                    );
                }
                (ImportMode::Append | ImportMode::Replace, _) => {
                    statements.push(format!("{head}{values}"));
                }
            }
        }
        Ok(statements)
    }

    /// The target columns, written as the values of `schema` (the source's, in plan order).
    fn target_columns(&self, schema: &Schema) -> Vec<Column<'_>> {
        self.columns
            .iter()
            .zip(schema.fields())
            .map(|(c, field)| Column {
                name: &c.target,
                kind: ValueKind::of(field.data_type()),
                default: None,
            })
            .collect()
    }

    /// The `INSERT … SELECT … FROM OPENJSON(…)` of `rows` (see [`bulk`]), whose values are of
    /// `schema`.
    fn bulk_insert(
        &self,
        dialect: Dialect,
        schema: &Schema,
        rows: &[Vec<Option<String>>],
    ) -> Result<String, ImportError> {
        let columns = self.target_columns(schema);
        let mut json = String::with_capacity(rows.len() * columns.len() * 12);
        json.push('[');
        for (r, row) in rows.iter().enumerate() {
            json.push_str(if r == 0 { "[" } else { ",[" });
            for (i, (column, value)) in columns.iter().zip(row).enumerate() {
                if i > 0 {
                    json.push(',');
                }
                let Some(text) = value else {
                    json.push_str("null");
                    continue;
                };
                // The literal checks the value, and is what a bare number or flag takes.
                let shown = literal(column, dialect, Some(text))?;
                let value = match column.kind {
                    ValueKind::Text | ValueKind::Timestamp { .. } => text.as_str(),
                    _ if shown.starts_with(['\'', 'N']) => text.as_str(),
                    _ => shown.as_str(),
                };
                json.push_str(&serde_json::Value::from(value).to_string());
            }
            json.push(']');
        }
        json.push(']');
        let mut with = Vec::with_capacity(columns.len());
        let mut select = Vec::with_capacity(columns.len());
        for (i, column) in columns.iter().enumerate() {
            with.push(format!("c{i} NVARCHAR(MAX) '$[{i}]'"));
            select.push(match column.kind {
                // `0x…`, as the literal writes binary values.
                ValueKind::Binary => format!("CONVERT(VARBINARY(MAX), c{i}, 1)"),
                _ => format!("c{i}"),
            });
        }
        let names: Vec<_> = columns
            .iter()
            .map(|c| dialect.quote_ident(c.name))
            .collect();
        Ok(format!(
            "INSERT INTO {} ({}) SELECT {} FROM OPENJSON({}) WITH ({})",
            self.table(dialect),
            names.join(", "),
            select.join(", "),
            dialect.quote_literal(&json),
            with.join(", ")
        ))
    }

    /// The temporary table an upsert goes through in MySQL, whose `ON DUPLICATE KEY UPDATE`
    /// fires on any unique key: a row whose other unique column exists would update that row.
    fn staging(&self, dialect: Dialect) -> Option<String> {
        (dialect == Dialect::MySql && self.mode() == ImportMode::Upsert)
            .then(|| dialect.qualified(self.schema.as_deref(), "platypus_import_staging"))
    }

    /// Creates the empty staging table, after dropping one a failed import may have left.
    fn staging_sql(&self, dialect: Dialect) -> Option<[String; 2]> {
        let staging = self.staging(dialect)?;
        let columns: Vec<_> = self
            .columns
            .iter()
            .map(|c| dialect.quote_ident(&c.target))
            .collect();
        Some([
            format!("DROP TEMPORARY TABLE IF EXISTS {staging}"),
            format!(
                "CREATE TEMPORARY TABLE {staging} AS SELECT {} FROM {} WHERE 1 = 0",
                columns.join(", "),
                self.table(dialect)
            ),
        ])
    }

    /// MySQL's upsert of `values` through `staging`: update the rows with the key, insert the
    /// others, so that a duplicate on another unique key fails as an insert does.
    fn staged_upsert(
        &self,
        dialect: Dialect,
        staging: &str,
        names: &[&str],
        values: &str,
    ) -> [String; 4] {
        let table = self.table(dialect);
        let q = |c: &str| dialect.quote_ident(c).into_owned();
        let all: Vec<_> = names.iter().map(|c| q(c)).collect();
        let on: Vec<_> = self
            .keys
            .iter()
            .map(|k| format!("t.{0} = s.{0}", q(k)))
            .collect();
        let on = on.join(" AND ");
        let set: Vec<_> = names
            .iter()
            .filter(|c| !self.keys.iter().any(|k| k == *c))
            .map(|c| format!("t.{0} = s.{0}", q(c)))
            .collect();
        // Without other columns there is nothing to update: `SET` of a key to itself.
        let set = if set.is_empty() {
            format!("t.{0} = s.{0}", q(&self.keys[0]))
        } else {
            set.join(", ")
        };
        let list = all.join(", ");
        let picked: Vec<_> = all.iter().map(|c| format!("s.{c}")).collect();
        [
            format!("INSERT INTO {staging} ({list}) VALUES {values}"),
            format!("UPDATE {table} t JOIN {staging} s ON {on} SET {set}"),
            format!(
                "INSERT INTO {table} ({list}) SELECT {} FROM {staging} s \
                 WHERE NOT EXISTS (SELECT 1 FROM {table} t WHERE {on})",
                picked.join(", ")
            ),
            format!("DELETE FROM {staging}"),
        ]
    }
}

/// ` DEFAULT d` of a created column of type `declared`: MySQL takes one of a TEXT, BLOB or
/// JSON column only as an expression, in parentheses.
fn default_clause(dialect: Dialect, declared: &str, default: &str) -> String {
    let upper = declared.to_ascii_uppercase();
    if dialect == Dialect::MySql && ["TEXT", "BLOB", "JSON"].iter().any(|t| upper.contains(t)) {
        format!(" DEFAULT ({default})")
    } else {
        format!(" DEFAULT {default}")
    }
}

/// The column type in `target` that holds the values of `field`: from its Arrow type, or from
/// the type the source declares where Arrow only has text (SQLite's columns take their type
/// from their values, so an empty result has none).
#[must_use]
pub fn target_type(field: &Field, target: Dialect) -> String {
    let declared = matches!(
        field.data_type(),
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View | DataType::Null
    )
    .then(|| field.metadata().get(META_DB_TYPE))
    .flatten();
    // Text of a bounded length keeps it: MySQL cannot index (or key) TEXT.
    if let Some(n) = declared.and_then(|t| text_length(t)) {
        return match target {
            Dialect::SqlServer => format!("NVARCHAR({n})"),
            Dialect::Sqlite => "TEXT".to_owned(),
            _ => format!("VARCHAR({n})"),
        };
    }
    let declared = declared.and_then(|t| declared_type(t));
    target.column_type(declared.as_ref().unwrap_or(field.data_type()))
}

/// The length of a declared `varchar(n)` or `char(n)`.
pub(crate) fn text_length(name: &str) -> Option<u32> {
    let (base, args) = name.split_once('(')?;
    let base = base.trim().to_ascii_lowercase();
    let text = [
        "varchar",
        "character varying",
        "nvarchar",
        "char",
        "character",
        "nchar",
    ];
    text.contains(&base.as_str())
        .then(|| args.trim_end_matches(')').trim().parse().ok())
        .flatten()
}

/// The Arrow type of a declared column type, for the types other databases share.
fn declared_type(name: &str) -> Option<DataType> {
    let name = name.trim().to_ascii_uppercase();
    let (base, args) = match name.split_once('(') {
        Some((base, args)) => (base.trim(), Some(args.trim_end_matches(')'))),
        None => (name.as_str(), None),
    };
    Some(match base {
        "INT" | "INTEGER" | "BIGINT" | "SMALLINT" | "TINYINT" | "MEDIUMINT" | "INT8" | "INT4" => {
            DataType::Int64
        }
        "REAL" | "FLOAT" | "DOUBLE" | "DOUBLE PRECISION" => DataType::Float64,
        "BOOLEAN" | "BOOL" => DataType::Boolean,
        "DATE" => DataType::Date32,
        "DATETIME" | "TIMESTAMP" => DataType::Timestamp(TimeUnit::Microsecond, None),
        "DECIMAL" | "NUMERIC" => {
            let (p, s) = args?.split_once(',').unwrap_or((args?, "0"));
            let p: u8 = p.trim().parse().ok()?;
            let s: i8 = s.trim().parse().ok()?;
            DataType::Decimal128(p.min(38), s)
        }
        _ => return None,
    })
}

/// Text `literal` for a column declared `declared` (a `uuid`, a `json`) in standard SQL,
/// whose Trino converts no text to another type, not even on INSERT: a literal of the
/// declared type, `uuid '…'`, or a `CAST` for a type with arguments. Elsewhere, and for text
/// columns, `literal` itself.
fn typed_text(dialect: Dialect, declared: Option<&str>, literal: String) -> String {
    let Some(declared) = declared.map(str::trim).filter(|d| !d.is_empty()) else {
        return literal;
    };
    let lower = declared.to_ascii_lowercase();
    let text = ["char", "text", "string", "clob"]
        .iter()
        .any(|t| lower.contains(t));
    if dialect != Dialect::Ansi || text {
        return literal;
    }
    if declared
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        format!("{declared} {literal}")
    } else {
        format!("CAST({literal} AS {declared})")
    }
}

/// Significant digits a double keeps: SQLite reads a number literal with more as a double.
const DOUBLE_DIGITS: usize = 15;

/// The literal of `text` for `column`. PostgreSQL's decimal `NaN` is written as a float's
/// `NaN`, quoted; a decimal with more digits than a double keeps is quoted for SQLite, so a
/// text column holds every digit (a numeric one converts it as it would the number).
pub(crate) fn literal(
    column: &Column<'_>,
    dialect: Dialect,
    text: Option<&str>,
) -> Result<String, InvalidValue> {
    if matches!(column.kind, ValueKind::Decimal) && text == Some("NaN") {
        let float = Column {
            kind: ValueKind::Float,
            ..*column
        };
        return float.literal(dialect, text);
    }
    let literal = column.literal(dialect, text)?;
    let digits = || {
        literal
            .bytes()
            .filter(u8::is_ascii_digit)
            .skip_while(|&b| b == b'0')
            .count()
    };
    if dialect == Dialect::Sqlite && column.kind == ValueKind::Decimal && digits() > DOUBLE_DIGITS {
        return Ok(dialect.quote_literal(&literal));
    }
    Ok(literal)
}

/// The values of `batch` as text, row by row, `None` for NULL: what an import writes. Lists,
/// structs and maps are JSON, which every target's text column can hold and give back.
pub fn rows_text(batch: &RecordBatch) -> Result<Vec<Vec<Option<String>>>, ArrowError> {
    let options = FormatOptions::default();
    let schema = batch.schema();
    let mut columns: Vec<Vec<Option<String>>> = Vec::with_capacity(batch.num_columns());
    for (field, array) in schema.fields().iter().zip(batch.columns()) {
        if let Some(json) = json_texts(array.as_ref()) {
            columns.push(json);
            continue;
        }
        // One formatter per column, not per value.
        let f = special::column_formatter(field, array.as_ref(), &options)?;
        columns.push(
            (0..array.len())
                .map(|row| (!array.is_null(row)).then(|| f.value(row).to_string()))
                .collect(),
        );
    }
    let mut columns: Vec<_> = columns.into_iter().map(Vec::into_iter).collect();
    Ok((0..batch.num_rows())
        .map(|_| columns.iter_mut().map(|c| c.next().flatten()).collect())
        .collect())
}

/// The values of a list, struct or map `array` as JSON texts, `None` for NULL; `None` for
/// other arrays, or one Arrow cannot write as JSON.
pub(crate) fn json_texts(array: &dyn Array) -> Option<Vec<Option<String>>> {
    if !matches!(
        array.data_type(),
        DataType::List(_)
            | DataType::LargeList(_)
            | DataType::FixedSizeList(..)
            | DataType::Struct(_)
            | DataType::Map(..)
    ) {
        return None;
    }
    let field = Field::new("v", array.data_type().clone(), true);
    let column = arrow_array::make_array(array.to_data());
    let batch = RecordBatch::try_new(Arc::new(Schema::new(vec![field])), vec![column]).ok()?;
    let mut out = Vec::new();
    let mut writer = arrow_json::WriterBuilder::new()
        .with_explicit_nulls(true)
        .build::<_, arrow_json::writer::LineDelimited>(&mut out);
    writer.write(&batch).ok()?;
    writer.finish().ok()?;
    drop(writer);
    let text = String::from_utf8(out).ok()?;
    // Each line is `{"v":<value>}`: the value as written, no number parsed again.
    let mut values = Vec::with_capacity(array.len());
    for (row, line) in text.lines().enumerate() {
        let value = line.strip_prefix("{\"v\":")?.strip_suffix('}')?;
        values.push((!array.is_null(row)).then(|| value.to_owned()));
    }
    (values.len() == array.len()).then_some(values)
}

/// `rows` with only the last of each key (the values at `at`), in their order; a row with a
/// NULL key matches none and stays.
fn last_per_key(rows: Vec<Vec<Option<String>>>, at: &[usize]) -> Vec<Vec<Option<String>>> {
    let mut seen = std::collections::HashSet::new();
    let mut kept: Vec<_> = rows
        .into_iter()
        .rev()
        .filter(|row| {
            let key: Option<Vec<&String>> = at.iter().map(|&i| row[i].as_ref()).collect();
            key.is_none_or(|k| seen.insert(k.into_iter().cloned().collect::<Vec<_>>()))
        })
        .collect();
    kept.reverse();
    kept
}

/// The rows of the query `sql`, as text; none for a statement without rows.
pub(crate) async fn query_rows(
    conn: &dyn Connection,
    sql: &str,
) -> Result<Vec<Vec<Option<String>>>, ImportError> {
    let ExecResult::Rows(mut rows) = conn.execute(sql, &ExecOptions::default()).await? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    while let Some(batch) = rows.next().await {
        out.extend(rows_text(&batch?)?);
    }
    Ok(out)
}

/// The first value of the query `sql`: none for no rows or NULL.
pub(crate) async fn query_value(
    conn: &dyn Connection,
    sql: &str,
) -> Result<Option<String>, ImportError> {
    Ok(query_rows(conn, sql)
        .await?
        .into_iter()
        .next()
        .and_then(|mut r| r.swap_remove(0)))
}

/// The schema an unqualified `CREATE TABLE` creates its table in; `None` where it cannot be
/// asked.
pub(crate) async fn current_schema(conn: &dyn Connection, dialect: Dialect) -> Option<String> {
    let sql = match dialect {
        Dialect::Postgres | Dialect::DuckDb => "SELECT current_schema()",
        Dialect::MySql => "SELECT DATABASE()",
        Dialect::SqlServer => "SELECT SCHEMA_NAME()",
        Dialect::Oracle => "SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM dual",
        Dialect::Sqlite => return Some("main".to_owned()),
        _ => return None,
    };
    query_value(conn, sql).await.ok().flatten()
}

/// Whether rows go to `target` as one JSON text per [`BULK_ROWS`], which SQL Server reads
/// with `OPENJSON` (compatibility level 130 and later): it compiles a long `VALUES` list of
/// literals ten times slower than it inserts the rows.
async fn bulk(target: &dyn Connection, dialect: Dialect) -> bool {
    if dialect != Dialect::SqlServer {
        return false;
    }
    let level = "SELECT compatibility_level FROM sys.databases WHERE name = DB_NAME()";
    query_value(target, level)
        .await
        .ok()
        .flatten()
        .and_then(|l| l.trim().parse::<u32>().ok())
        .is_some_and(|l| l >= 130)
}

/// The query that reads every document of `collection` (of database `db`, else the
/// session's) from MongoDB, `limit` of them at most.
#[must_use]
pub fn mongo_find(db: Option<&str>, collection: &str, limit: Option<u64>) -> String {
    let find = format!("{}.find({{}})", crate::fake::collection_ref(db, collection));
    match limit {
        Some(n) => format!("{find}.limit({n})"),
        None => find,
    }
}

/// The query of every row of `table` (of `schema`, else the session's) on a database of
/// `driver`, `limit` rows at most: SQL, or MongoDB's `find`.
#[must_use]
pub fn select_all(
    driver: DriverKind,
    (schema, table): (Option<&str>, &str),
    limit: Option<u64>,
) -> String {
    if driver == DriverKind::Mongo {
        return mongo_find(schema, table, limit);
    }
    let dialect = Dialect::from(driver);
    let sql = format!("SELECT * FROM {}", dialect.qualified(schema, table));
    match limit {
        Some(n) => dialect.limited(&sql, n),
        None => sql,
    }
}

/// `rows` with the columns named `names`, in that order: a NULL column for a name it has
/// not (a field no document holds).
fn project(rows: RecordBatchStream, names: &[String]) -> RecordBatchStream {
    let source = rows.schema();
    let at: Vec<Option<usize>> = names
        .iter()
        .map(|n| source.fields().iter().position(|f| f.name() == n))
        .collect();
    let fields: Vec<Field> = names
        .iter()
        .zip(&at)
        .map(|(name, i)| match i {
            Some(i) => source.field(*i).clone(),
            None => Field::new(name, DataType::Utf8, true),
        })
        .collect();
    let schema = Arc::new(Schema::new(fields));
    let projected = Arc::clone(&schema);
    let batches = rows.map(move |batch| {
        let batch = batch?;
        let columns = at
            .iter()
            .map(|i| match i {
                Some(i) => Arc::clone(batch.column(*i)),
                None => arrow_array::new_null_array(&DataType::Utf8, batch.num_rows()),
            })
            .collect();
        RecordBatch::try_new(Arc::clone(&projected), columns)
            .map_err(|e| platypus_core::Error::Other(e.to_string()))
    });
    RecordBatchStream::new(schema, batches.boxed())
}

/// [`import`] of the rows `query` reads on `source`, its columns taken by the names of the
/// plan's sources: a MongoDB `find`, whose fields come in the documents' order.
pub async fn import_query(
    source: &dyn Connection,
    query: &str,
    target: (&dyn Connection, Dialect),
    plan: &ImportPlan,
    progress: impl FnMut(u64) -> bool,
) -> Result<u64, ImportError> {
    plan.check()?;
    let options = ExecOptions {
        batch_size: ROWS_PER_INSERT,
        ..ExecOptions::default()
    };
    let ExecResult::Rows(rows) = source.execute(query, &options).await? else {
        return Err(ImportError::Plan("the source returned no rows".to_owned()));
    };
    let names: Vec<String> = plan.columns.iter().map(|c| c.source.clone()).collect();
    import_rows(project(rows, &names), target, plan, progress).await
}

async fn exec(conn: &dyn Connection, sql: &str) -> Result<(), platypus_core::Error> {
    // Dropping a stream of rows ends the statement.
    conn.execute(sql, &ExecOptions::default()).await.map(|_| ())
}

/// Copies the rows of `plan` from `source` (a DuckDB connection on a file, or another
/// database) to `target`, a connection of its own; each comes with its dialect. `progress`
/// gets the rows written so far and returns false to stop, which rolls back like a failure.
/// Returns how many rows were imported.
pub async fn import(
    (source, source_dialect): (&dyn Connection, Dialect),
    target: (&dyn Connection, Dialect),
    plan: &ImportPlan,
    progress: impl FnMut(u64) -> bool,
) -> Result<u64, ImportError> {
    plan.check()?;
    let options = ExecOptions {
        batch_size: ROWS_PER_INSERT,
        ..ExecOptions::default()
    };
    let sql = plan.source_sql(source_dialect);
    let ExecResult::Rows(rows) = source.execute(&sql, &options).await? else {
        return Err(ImportError::Plan("the source returned no rows".to_owned()));
    };
    import_rows(rows, target, plan, progress).await
}

/// Writes `rows`, whose columns are the plan's sources in order, as [`import`] does.
pub async fn import_rows(
    mut rows: RecordBatchStream,
    (target, dialect): (&dyn Connection, Dialect),
    plan: &ImportPlan,
    mut progress: impl FnMut(u64) -> bool,
) -> Result<u64, ImportError> {
    plan.check()?;
    // An explicit BEGIN, as `dml::apply` does: it works in either commit mode, and opens the
    // transaction before DDL too. ClickHouse has none: the rows written stay on failure.
    let transaction = target.capabilities().transactions;
    if transaction {
        exec(target, "BEGIN").await?;
    }
    // The table a failed import created, by its full name: an unqualified one could find
    // another table of that name further down the search path once a rollback undid it.
    let mut created: Option<String> = None;
    let outcome = async {
        if plan.create {
            let schema = match &plan.schema {
                Some(schema) => Some(schema.clone()),
                None => current_schema(target, dialect).await,
            };
            exec(target, &plan.create_sql(dialect)?).await?;
            created = Some(dialect.qualified(schema.as_deref(), &plan.table));
        }
        if plan.mode() == ImportMode::Replace {
            // ClickHouse deletes no rows without a WHERE, and has no transaction to undo it.
            let sql = if dialect == Dialect::ClickHouse {
                format!("TRUNCATE TABLE {}", plan.table(dialect))
            } else {
                format!("DELETE FROM {}", plan.table(dialect))
            };
            exec(target, &sql).await?;
        }
        // Temporary tables commit nothing in MySQL.
        for sql in plan.staging_sql(dialect).into_iter().flatten() {
            exec(target, &sql).await?;
        }
        if let Some(sql) = plan.identity_insert(dialect, true) {
            exec(target, &sql).await?;
        }
        let bulk = plan.mode() != ImportMode::Upsert && bulk(target, dialect).await;
        let mut pending: Vec<Vec<Option<String>>> = Vec::new();
        let mut schema = rows.schema();
        let mut written = 0;
        while let Some(batch) = rows.next().await {
            let batch = batch?;
            if bulk {
                schema = batch.schema();
                pending.extend(rows_text(&batch)?);
                if pending.len() >= BULK_ROWS {
                    let sql = plan.bulk_insert(dialect, &schema, &pending)?;
                    exec(target, &sql).await?;
                    pending.clear();
                }
            } else {
                for sql in plan.insert_sql(dialect, &batch)? {
                    exec(target, &sql).await?;
                }
            }
            written += batch.num_rows() as u64;
            if !progress(written) {
                return Err(ImportError::Cancelled);
            }
        }
        if !pending.is_empty() {
            let sql = plan.bulk_insert(dialect, &schema, &pending)?;
            exec(target, &sql).await?;
        }
        if let Some(staging) = plan.staging(dialect) {
            exec(target, &format!("DROP TEMPORARY TABLE {staging}")).await?;
        }
        if let Some(sql) = plan.identity_insert(dialect, false) {
            exec(target, &sql).await?;
        }
        if transaction {
            target.commit().await?;
        }
        Ok(written)
    }
    .await;
    if outcome.is_err() {
        // The first error explains more than a failed cleanup.
        if transaction {
            let _ = target.rollback().await;
        }
        // Most databases' rollback undid the CREATE already, and the DROP fails; MySQL,
        // Oracle and CockroachDB keep the table. In manual-commit mode it opens a
        // transaction, ended here.
        if let Some(table) = created {
            let _ = exec(target, &format!("DROP TABLE {table}")).await;
            let _ = target.rollback().await;
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{Int64Array, StringArray};
    use arrow_schema::{DataType, Field, Schema};

    use super::*;

    fn plan(create: bool) -> ImportPlan {
        ImportPlan {
            source_table: "sales".to_owned(),
            source_schema: None,
            schema: Some("public".to_owned()),
            table: "Sales".to_owned(),
            create,
            mode: ImportMode::Append,
            keys: Vec::new(),
            primary_key: Vec::new(),
            identity: None,
            constraints: Vec::new(),
            columns: vec![
                ImportColumn {
                    source: "id".to_owned(),
                    target: "id".to_owned(),
                    data_type: Some("BIGINT".to_owned()),
                    not_null: false,
                    default: None,
                },
                ImportColumn {
                    source: "item name".to_owned(),
                    target: "item".to_owned(),
                    data_type: Some("TEXT".to_owned()),
                    not_null: false,
                    default: None,
                },
            ],
        }
    }

    fn batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("item name", DataType::Utf8, true),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int64Array::from(vec![Some(1), None])),
                Arc::new(StringArray::from(vec![Some("it's"), Some("ink")])),
            ],
        )
        .unwrap()
    }

    #[test]
    fn statements_of_a_plan() {
        let p = plan(true);
        assert_eq!(
            p.create_sql(Dialect::Postgres).unwrap(),
            r#"CREATE TABLE public."Sales" (id BIGINT, item TEXT)"#
        );
        let mut ch = plan(true);
        ch.columns[0].data_type = Some("Int64".to_owned());
        ch.columns[1].data_type = Some("LowCardinality(Nullable(String))".to_owned());
        assert_eq!(
            ch.create_sql(Dialect::ClickHouse).unwrap(),
            "CREATE TABLE public.Sales (id Int64 NULL, item LowCardinality(Nullable(String)))"
        );
        assert_eq!(
            p.source_sql(Dialect::DuckDb),
            r#"SELECT id, "item name" FROM sales"#
        );
        assert_eq!(
            p.insert_sql(Dialect::Postgres, &batch()).unwrap(),
            [r#"INSERT INTO public."Sales" (id, item) VALUES (1, 'it''s'), (NULL, 'ink')"#]
        );
    }

    #[test]
    fn standard_sql_types_text_of_other_columns() {
        let mut p = plan(false);
        p.columns[1].data_type = Some("uuid".to_owned());
        assert_eq!(
            p.insert_sql(Dialect::Ansi, &batch()).unwrap(),
            [
                r#"INSERT INTO "public"."Sales" ("id", "item") VALUES (1, uuid 'it''s'), (NULL, uuid 'ink')"#
            ]
        );
        p.columns[1].data_type = Some("decimal(5, 2)".to_owned());
        assert!(
            p.insert_sql(Dialect::Ansi, &batch()).unwrap()[0]
                .contains("CAST('ink' AS decimal(5, 2))")
        );
        // Text stays text, and other databases convert it themselves.
        p.columns[1].data_type = Some("varchar(10)".to_owned());
        assert!(p.insert_sql(Dialect::Ansi, &batch()).unwrap()[0].contains("(NULL, 'ink')"));
        p.columns[1].data_type = Some("uuid".to_owned());
        assert!(p.insert_sql(Dialect::Postgres, &batch()).unwrap()[0].contains("(NULL, 'ink')"));
    }

    /// A batch of ids and names.
    fn rows(ids: &[i64], names: &[&str]) -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("item name", DataType::Utf8, true),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int64Array::from(ids.to_vec())),
                Arc::new(StringArray::from(names.to_vec())),
            ],
        )
        .unwrap()
    }

    #[test]
    fn upserts_keep_the_last_row_of_a_key_everywhere() {
        let mut p = plan(false);
        p.mode = ImportMode::Upsert;
        p.keys = vec!["id".to_owned()];
        let batch = rows(&[2, 3, 3], &["B", "c", "C"]);
        let sql = p.insert_sql(Dialect::Postgres, &batch).unwrap();
        assert!(sql[0].contains("VALUES (2, 'B'), (3, 'C')"), "{sql:?}");
        let sql = p.insert_sql(Dialect::SqlServer, &batch).unwrap();
        assert!(!sql[0].contains("N'c'"), "{sql:?}");
    }

    #[test]
    fn sql_server_takes_rows_as_one_json_text() {
        let p = plan(false);
        let batch = rows(&[1, 2], &["it's", "\"x\""]);
        let text = rows_text(&batch).unwrap();
        let sql = p
            .bulk_insert(Dialect::SqlServer, &batch.schema(), &text)
            .unwrap();
        assert_eq!(
            sql,
            "INSERT INTO [public].Sales (id, item) SELECT c0, c1 FROM \
             OPENJSON(N'[[\"1\",\"it''s\"],[\"2\",\"\\\"x\\\"\"]]') \
             WITH (c0 NVARCHAR(MAX) '$[0]', c1 NVARCHAR(MAX) '$[1]')"
        );
        // A value the column cannot take fails as a literal would.
        let mut wrong = plan(false);
        wrong.columns.swap(0, 1);
        let bad = rows_text(&rows(&[1], &["x"])).unwrap();
        let swapped = Schema::new(vec![
            Field::new("item name", DataType::Utf8, true),
            Field::new("id", DataType::Int64, true),
        ]);
        let reversed: Vec<Vec<Option<String>>> = bad
            .into_iter()
            .map(|r| r.into_iter().rev().collect())
            .collect();
        assert!(
            wrong
                .bulk_insert(Dialect::SqlServer, &swapped, &reversed)
                .is_ok()
        );
        let typed = Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("item name", DataType::Int64, true),
        ]);
        assert!(
            p.bulk_insert(
                Dialect::SqlServer,
                &typed,
                &[vec![Some("1".to_owned()), Some("x".to_owned())]]
            )
            .is_err()
        );
    }

    #[test]
    fn nested_values_are_imported_as_json() {
        use arrow_array::builder::{ListBuilder, StringBuilder};
        let mut tags = ListBuilder::new(StringBuilder::new());
        tags.values().append_value("a, b");
        tags.values().append_value("c");
        tags.append(true);
        tags.append(false);
        let schema = Arc::new(Schema::new(vec![Field::new(
            "tags",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            true,
        )]));
        let batch = RecordBatch::try_new(schema, vec![Arc::new(tags.finish())]).unwrap();
        assert_eq!(
            rows_text(&batch).unwrap(),
            [vec![Some(r#"["a, b","c"]"#.to_owned())], vec![None]]
        );
    }

    #[test]
    fn long_decimals_stay_text_in_sqlite() {
        let column = Column {
            name: "n",
            kind: ValueKind::Decimal,
            default: None,
        };
        let long = "12345678901234567.123456789";
        assert_eq!(
            literal(&column, Dialect::Sqlite, Some(long)).unwrap(),
            format!("'{long}'")
        );
        assert_eq!(
            literal(&column, Dialect::Sqlite, Some("1.50")).unwrap(),
            "1.50"
        );
        assert_eq!(
            literal(&column, Dialect::Postgres, Some(long)).unwrap(),
            long
        );
    }

    #[test]
    fn reads_every_row_of_sql_and_mongodb() {
        assert_eq!(
            select_all(DriverKind::Postgres, (Some("s"), "T"), Some(5)),
            "SELECT * FROM s.\"T\" LIMIT 5"
        );
        assert_eq!(
            select_all(DriverKind::Mongo, (None, "customers"), None),
            "db.getCollection(\"customers\").find({})"
        );
        assert_eq!(
            mongo_find(Some("shop"), "c", Some(50)),
            "db.getSiblingDB(\"shop\").getCollection(\"c\").find({}).limit(50)"
        );
    }

    #[test]
    fn mysql_text_defaults_are_expressions() {
        let mut p = plan(true);
        p.columns[1].default = Some("'x'".to_owned());
        p.columns[1].not_null = true;
        p.primary_key = vec!["id".to_owned()];
        assert_eq!(
            p.create_sql(Dialect::MySql).unwrap(),
            "CREATE TABLE public.Sales (id BIGINT, item TEXT DEFAULT ('x') NOT NULL, PRIMARY KEY (id))"
        );
        assert_eq!(
            p.create_sql(Dialect::Sqlite).unwrap(),
            "CREATE TABLE public.Sales (id BIGINT, item TEXT DEFAULT 'x' NOT NULL, PRIMARY KEY (id))"
        );
    }

    #[test]
    fn upserts_on_the_key() {
        let mut p = plan(false);
        p.mode = ImportMode::Upsert;
        assert!(matches!(p.check(), Err(ImportError::Plan(_))), "no key");
        p.keys = vec!["nope".to_owned()];
        assert!(
            matches!(p.check(), Err(ImportError::Plan(_))),
            "key not imported"
        );
        p.keys = vec!["id".to_owned()];
        p.check().unwrap();
        assert_eq!(
            p.insert_sql(Dialect::Postgres, &batch()).unwrap(),
            [
                "INSERT INTO public.\"Sales\" (id, item)\nVALUES (1, 'it''s'), (NULL, 'ink')\n\
                 ON CONFLICT (id) DO UPDATE SET item = EXCLUDED.item"
            ]
        );
        // A created table has nothing to update.
        p.create = true;
        assert!(
            p.insert_sql(Dialect::Postgres, &batch()).unwrap()[0]
                .starts_with("INSERT INTO public.\"Sales\" (id, item) VALUES")
        );
    }

    #[test]
    fn types_of_text_columns_from_their_declared_type() {
        let field = |data_type: DataType, declared: &str| {
            Field::new("c", data_type, true).with_metadata(std::collections::HashMap::from([(
                META_DB_TYPE.to_owned(),
                declared.to_owned(),
            )]))
        };
        let d = Dialect::Postgres;
        assert_eq!(
            target_type(&field(DataType::Utf8, "DECIMAL(10, 2)"), d),
            "DECIMAL(10, 2)"
        );
        assert_eq!(target_type(&field(DataType::Utf8, "INT"), d), "BIGINT");
        assert_eq!(target_type(&field(DataType::Utf8, "uuid"), d), "TEXT");
        assert_eq!(
            target_type(&field(DataType::Utf8, "varchar(20)"), d),
            "VARCHAR(20)"
        );
        assert_eq!(
            target_type(&field(DataType::Utf8, "varchar(20)"), Dialect::SqlServer),
            "NVARCHAR(20)"
        );
        assert_eq!(
            target_type(&field(DataType::Utf8, "nvarchar(max)"), d),
            "TEXT"
        );
        assert_eq!(
            target_type(&field(DataType::Utf8, "numeric"), d),
            "TEXT",
            "no precision"
        );
        // A typed column keeps its type.
        assert_eq!(
            target_type(&field(DataType::Float64, "DECIMAL(10, 2)"), d),
            "DOUBLE PRECISION"
        );
    }

    #[test]
    fn keeps_the_last_row_of_a_key() {
        let row = |k: Option<&str>, v: &str| vec![k.map(str::to_owned), Some(v.to_owned())];
        let rows = vec![
            row(Some("1"), "a"),
            row(None, "b"),
            row(Some("2"), "c"),
            row(Some("1"), "d"),
            row(None, "e"),
        ];
        assert_eq!(
            last_per_key(rows, &[0]),
            [
                row(None, "b"),
                row(Some("2"), "c"),
                row(Some("1"), "d"),
                row(None, "e")
            ]
        );
    }

    #[test]
    fn writes_decimal_nan() {
        let column = Column {
            name: "n",
            kind: ValueKind::Decimal,
            default: None,
        };
        assert_eq!(
            literal(&column, Dialect::Postgres, Some("NaN")).unwrap(),
            "'NaN'"
        );
        assert_eq!(
            literal(&column, Dialect::Postgres, Some("1.50")).unwrap(),
            "1.50"
        );
        assert!(literal(&column, Dialect::Postgres, Some("x")).is_err());
    }

    #[test]
    fn creating_needs_every_type() {
        let mut p = plan(true);
        p.columns[1].data_type = Some(" ".to_owned());
        assert!(matches!(
            p.create_sql(Dialect::MySql),
            Err(ImportError::Plan(_))
        ));
    }
}
