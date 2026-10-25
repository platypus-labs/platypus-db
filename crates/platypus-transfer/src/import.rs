//! Copy of a table into a table of another connection, optionally created from the source
//! types: a data file's (read through a DuckDB connection on the file) or another
//! database's.
//!
//! Rows travel as multi-row `INSERT` statements with literals of the source column types, in
//! one transaction: a failed import leaves the target as it was, and a table it created is
//! dropped again (MySQL commits DDL on its own). Rows can also replace the table's or update
//! the rows with the same key.

use arrow_array::RecordBatch;
use arrow_cast::display::{ArrayFormatter, FormatOptions};
use arrow_schema::{ArrowError, DataType, Field, TimeUnit};
use futures::StreamExt;
use platypus_core::{Connection, ExecOptions, ExecResult, META_DB_TYPE, RecordBatchStream};
use platypus_sql::Dialect;
use platypus_sql::dml::{Column, InvalidValue, ValueKind};
use serde::Deserialize;

/// Rows per `INSERT`: SQL Server takes at most 1000 in a `VALUES` list.
const ROWS_PER_INSERT: usize = 500;

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
                        .map_or_else(String::new, |d| format!(" DEFAULT {d}")),
                    if c.not_null { " NOT NULL" } else { "" }
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
            Dialect::DuckDb => declared.to_owned(),
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
        let schema = batch.schema();
        let columns: Vec<Column<'_>> = self
            .columns
            .iter()
            .zip(schema.fields())
            .map(|(c, field)| Column {
                name: &c.target,
                kind: ValueKind::of(field.data_type()),
                default: None,
            })
            .collect();
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
        let mut statements = Vec::new();
        for chunk in rows_text(batch)?.chunks(ROWS_PER_INSERT) {
            let values = chunk
                .iter()
                .map(|row| {
                    let literals = columns
                        .iter()
                        .zip(row)
                        .map(|(c, text)| c.literal(dialect, text.as_deref()))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(format!("({})", literals.join(", ")))
                })
                .collect::<Result<Vec<_>, ImportError>>()?
                .join(", ");
            match (self.mode(), self.staging(dialect)) {
                (ImportMode::Upsert, Some(staging)) => {
                    statements.extend(self.staged_upsert(dialect, &staging, &names, &values));
                }
                (ImportMode::Upsert, None) => {
                    statements.push(dialect.upsert_rows(&table, &names, &keys, &values));
                }
                (ImportMode::Append | ImportMode::Replace, _) => {
                    statements.push(format!("{head}{values}"));
                }
            }
        }
        Ok(statements)
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
fn text_length(name: &str) -> Option<u32> {
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

/// The values of `batch` as text, row by row, `None` for NULL: what an import writes.
pub fn rows_text(batch: &RecordBatch) -> Result<Vec<Vec<Option<String>>>, ArrowError> {
    let options = FormatOptions::default();
    // One formatter per column, not per value.
    let formatters = batch
        .columns()
        .iter()
        .map(|a| ArrayFormatter::try_new(a.as_ref(), &options))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((0..batch.num_rows())
        .map(|row| {
            formatters
                .iter()
                .zip(batch.columns())
                .map(|(f, a)| (!a.is_null(row)).then(|| f.value(row).to_string()))
                .collect()
        })
        .collect())
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
    // transaction before DDL too.
    exec(target, "BEGIN").await?;
    let mut created = false;
    let outcome = async {
        if plan.create {
            exec(target, &plan.create_sql(dialect)?).await?;
            created = true;
        }
        if plan.mode() == ImportMode::Replace {
            exec(target, &format!("DELETE FROM {}", plan.table(dialect))).await?;
        }
        // Temporary tables commit nothing in MySQL.
        for sql in plan.staging_sql(dialect).into_iter().flatten() {
            exec(target, &sql).await?;
        }
        if let Some(sql) = plan.identity_insert(dialect, true) {
            exec(target, &sql).await?;
        }
        let mut written = 0;
        while let Some(batch) = rows.next().await {
            let batch = batch?;
            for sql in plan.insert_sql(dialect, &batch)? {
                exec(target, &sql).await?;
            }
            written += batch.num_rows() as u64;
            if !progress(written) {
                return Err(ImportError::Cancelled);
            }
        }
        if let Some(staging) = plan.staging(dialect) {
            exec(target, &format!("DROP TEMPORARY TABLE {staging}")).await?;
        }
        if let Some(sql) = plan.identity_insert(dialect, false) {
            exec(target, &sql).await?;
        }
        target.commit().await?;
        Ok(written)
    }
    .await;
    if outcome.is_err() {
        // The first error explains more than a failed cleanup.
        let _ = target.rollback().await;
        if created {
            // Only MySQL still has the table; elsewhere the DROP fails. In manual-commit mode
            // it opens a transaction, ended here.
            let _ = exec(target, &format!("DROP TABLE {}", plan.table(dialect))).await;
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
    fn creating_needs_every_type() {
        let mut p = plan(true);
        p.columns[1].data_type = Some(" ".to_owned());
        assert!(matches!(
            p.create_sql(Dialect::MySql),
            Err(ImportError::Plan(_))
        ));
    }
}
