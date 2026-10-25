//! Import of a table read from a data file (a DuckDB connection on the file) into a table of
//! another connection, optionally created from the inferred types.
//!
//! Rows travel as multi-row `INSERT` statements with literals of the source column types, in
//! one transaction: a failed import leaves the target as it was, and a table it created is
//! dropped again (MySQL commits DDL on its own).

use arrow_array::RecordBatch;
use arrow_cast::display::{ArrayFormatter, FormatOptions};
use arrow_schema::ArrowError;
use futures::StreamExt;
use platypus_core::{Connection, ExecOptions, ExecResult};
use platypus_sql::Dialect;
use platypus_sql::dml::{Column, InvalidValue, ValueKind};
use serde::Deserialize;

/// Rows per `INSERT`: SQL Server takes at most 1000 in a `VALUES` list.
const ROWS_PER_INSERT: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ImportColumn {
    /// The column of the file's table.
    pub source: String,
    /// The column of the target table.
    pub target: String,
    /// The column type, used when the import creates the table.
    #[serde(default)]
    pub data_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ImportPlan {
    /// The table or view of the file's DuckDB database.
    pub source_table: String,
    pub schema: Option<String>,
    pub table: String,
    /// Create the table first, from the columns' `data_type`.
    pub create: bool,
    pub columns: Vec<ImportColumn>,
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
}

impl ImportPlan {
    fn table(&self, dialect: Dialect) -> String {
        dialect.qualified(self.schema.as_deref(), &self.table)
    }

    /// `CREATE TABLE` with the columns of the plan.
    pub fn create_sql(&self, dialect: Dialect) -> Result<String, ImportError> {
        let columns = self
            .columns
            .iter()
            .map(|c| match c.data_type.as_deref().map(str::trim) {
                Some(t) if !t.is_empty() => Ok(format!("{} {t}", dialect.quote_ident(&c.target))),
                _ => Err(ImportError::Plan(format!(
                    "column {} has no type",
                    c.target
                ))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(format!(
            "CREATE TABLE {} ({})",
            self.table(dialect),
            columns.join(", ")
        ))
    }

    /// The query of the source columns, in the file's DuckDB database.
    #[must_use]
    pub fn source_sql(&self) -> String {
        let d = Dialect::DuckDb;
        let columns: Vec<_> = self
            .columns
            .iter()
            .map(|c| d.quote_ident(&c.source))
            .collect();
        format!(
            "SELECT {} FROM {}",
            columns.join(", "),
            d.quote_ident(&self.source_table)
        )
    }

    /// The `INSERT` statements of `batch`, whose columns are the plan's in order.
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
        let head = format!(
            "INSERT INTO {} ({}) VALUES ",
            self.table(dialect),
            columns
                .iter()
                .map(|c| dialect.quote_ident(c.name))
                .collect::<Vec<_>>()
                .join(", ")
        );
        rows_text(batch)?
            .chunks(ROWS_PER_INSERT)
            .map(|chunk| {
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
                    .collect::<Result<Vec<_>, ImportError>>()?;
                Ok(format!("{head}{}", values.join(", ")))
            })
            .collect()
    }
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

async fn exec(conn: &dyn Connection, sql: &str) -> Result<(), platypus_core::Error> {
    // Dropping a stream of rows ends the statement.
    conn.execute(sql, &ExecOptions::default()).await.map(|_| ())
}

/// Copies the rows of `plan` from `source`, a DuckDB connection on the file, to `target`, a
/// connection of its own in `dialect`; `progress` gets the rows written so far. Returns how
/// many rows were imported.
pub async fn import(
    source: &dyn Connection,
    target: &dyn Connection,
    dialect: Dialect,
    plan: &ImportPlan,
    mut progress: impl FnMut(u64),
) -> Result<u64, ImportError> {
    if plan.columns.is_empty() {
        return Err(ImportError::Plan("no column to import".to_owned()));
    }
    // An explicit BEGIN, as `dml::apply` does: it works in either commit mode, and opens the
    // transaction before DDL too.
    exec(target, "BEGIN").await?;
    let mut created = false;
    let outcome = async {
        if plan.create {
            exec(target, &plan.create_sql(dialect)?).await?;
            created = true;
        }
        let options = ExecOptions {
            batch_size: ROWS_PER_INSERT,
            ..ExecOptions::default()
        };
        let ExecResult::Rows(mut rows) = source.execute(&plan.source_sql(), &options).await? else {
            return Err(ImportError::Plan("the file returned no rows".to_owned()));
        };
        let mut written = 0;
        while let Some(batch) = rows.next().await {
            let batch = batch?;
            for sql in plan.insert_sql(dialect, &batch)? {
                exec(target, &sql).await?;
            }
            written += batch.num_rows() as u64;
            progress(written);
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
            schema: Some("public".to_owned()),
            table: "Sales".to_owned(),
            create,
            columns: vec![
                ImportColumn {
                    source: "id".to_owned(),
                    target: "id".to_owned(),
                    data_type: Some("BIGINT".to_owned()),
                },
                ImportColumn {
                    source: "item name".to_owned(),
                    target: "item".to_owned(),
                    data_type: Some("TEXT".to_owned()),
                },
            ],
        }
    }

    #[test]
    fn statements_of_a_plan() {
        let p = plan(true);
        assert_eq!(
            p.create_sql(Dialect::Postgres).unwrap(),
            r#"CREATE TABLE public."Sales" (id BIGINT, item TEXT)"#
        );
        assert_eq!(p.source_sql(), r#"SELECT id, "item name" FROM sales"#);
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("item name", DataType::Utf8, true),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int64Array::from(vec![Some(1), None])),
                Arc::new(StringArray::from(vec![Some("it's"), Some("ink")])),
            ],
        )
        .unwrap();
        assert_eq!(
            p.insert_sql(Dialect::Postgres, &batch).unwrap(),
            [r#"INSERT INTO public."Sales" (id, item) VALUES (1, 'it''s'), (NULL, 'ink')"#]
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
