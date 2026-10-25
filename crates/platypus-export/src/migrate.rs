//! Migration of tables between databases: each is created in the target with its columns
//! mapped (types, NOT NULL, primary key) and its rows copied, then the foreign keys between
//! the copied tables are added, once every row is there.

use platypus_core::{Connection, ExecOptions, ExecResult, ForeignKeyMeta};
use platypus_sql::Dialect;

use crate::import::{ImportColumn, ImportError, ImportMode, ImportPlan, import, target_type};

/// What a migration did.
#[derive(Debug, Default)]
pub struct Migrated {
    pub rows: u64,
    /// The foreign keys left out, with why: the tables and rows are there anyway.
    pub skipped: Vec<String>,
}

/// One end of a migration: a connection, its dialect, and the schema of the tables.
pub type End<'a> = (&'a dyn Connection, Dialect, &'a str);

/// The plan that creates `table` in the target and copies its rows.
async fn plan_of(
    (source, dialect, schema): End<'_>,
    (target_dialect, target_schema): (Dialect, &str),
    table: &str,
) -> Result<(ImportPlan, Vec<ForeignKeyMeta>), ImportError> {
    let details = source.table_details(schema, table).await?;
    let sql = format!(
        "SELECT * FROM {} WHERE 1 = 0",
        dialect.qualified(Some(schema), table)
    );
    let ExecResult::Rows(rows) = source.execute(&sql, &ExecOptions::default()).await? else {
        return Err(ImportError::Plan(format!("{table} returned no rows")));
    };
    let fields = rows.schema();
    drop(rows);
    let not_null = |name: &str| {
        details
            .columns
            .iter()
            .any(|c| c.name == name && !c.nullable)
    };
    let plan = ImportPlan {
        source_table: table.to_owned(),
        source_schema: Some(schema.to_owned()),
        schema: Some(target_schema.to_owned()),
        table: table.to_owned(),
        create: true,
        mode: ImportMode::Append,
        keys: Vec::new(),
        primary_key: details
            .columns
            .iter()
            .filter(|c| c.primary_key)
            .map(|c| c.name.clone())
            .collect(),
        columns: fields
            .fields()
            .iter()
            .map(|f| ImportColumn {
                source: f.name().clone(),
                target: f.name().clone(),
                data_type: Some(target_type(f, target_dialect)),
                not_null: not_null(f.name()),
            })
            .collect(),
    };
    Ok((plan, details.foreign_keys))
}

/// `ALTER TABLE ... ADD CONSTRAINT ... FOREIGN KEY` of `fk` on `table`, in the target schema.
fn foreign_key_sql(dialect: Dialect, schema: &str, table: &str, fk: &ForeignKeyMeta) -> String {
    let list = |names: &[String]| {
        names
            .iter()
            .map(|n| dialect.quote_ident(n).into_owned())
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
        dialect.qualified(Some(schema), table),
        dialect.quote_ident(&fk.name),
        list(&fk.columns),
        dialect.qualified(Some(schema), &fk.ref_table),
        list(&fk.ref_columns)
    )
}

async fn exec(conn: &dyn Connection, sql: &str) -> Result<(), platypus_core::Error> {
    conn.execute(sql, &ExecOptions::default()).await.map(|_| ())
}

/// Copies `tables` of the source schema into new tables of the target schema, one table per
/// transaction, then adds their foreign keys. `progress` gets the rows copied so far and
/// returns false to stop. A failed table stops the migration; the tables copied before it
/// stay.
pub async fn migrate(
    source: End<'_>,
    (target, dialect, schema): End<'_>,
    tables: &[String],
    mut progress: impl FnMut(u64) -> bool,
) -> Result<Migrated, ImportError> {
    let mut done = Migrated::default();
    let mut keys = Vec::new();
    for table in tables {
        let (plan, fks) = plan_of(source, (dialect, schema), table).await?;
        let base = done.rows;
        done.rows += import((source.0, source.1), (target, dialect), &plan, |n| {
            progress(base + n)
        })
        .await
        .map_err(|e| match e {
            ImportError::Cancelled => e,
            e => ImportError::Plan(format!("{table}: {e}")),
        })?;
        keys.extend(fks.into_iter().map(|fk| (table, fk)));
    }
    for (table, fk) in keys {
        let name = format!("{table}.{}", fk.name);
        if !tables.contains(&fk.ref_table) {
            done.skipped
                .push(format!("{name}: {} was not copied", fk.ref_table));
        } else if matches!(dialect, Dialect::Sqlite | Dialect::DuckDb) {
            // They declare foreign keys only with the table.
            let db = if dialect == Dialect::Sqlite {
                "SQLite"
            } else {
                "DuckDB"
            };
            done.skipped
                .push(format!("{name}: {db} cannot add it to an existing table"));
        } else {
            let sql = foreign_key_sql(dialect, schema, table, &fk);
            // Each in a transaction of its own, so that one failure does not abort the rest.
            exec(target, "BEGIN").await?;
            match exec(target, &sql).await {
                Ok(()) => target.commit().await?,
                Err(e) => {
                    let _ = target.rollback().await;
                    done.skipped.push(format!("{name}: {e}"));
                }
            }
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_keys_point_at_the_target_schema() {
        let fk = ForeignKeyMeta {
            name: "orders_customer".to_owned(),
            columns: vec!["customer_id".to_owned()],
            ref_schema: "demo".to_owned(),
            ref_table: "Customers".to_owned(),
            ref_columns: vec!["id".to_owned()],
        };
        assert_eq!(
            foreign_key_sql(Dialect::Postgres, "copy", "orders", &fk),
            r#"ALTER TABLE copy.orders ADD CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES copy."Customers" (id)"#
        );
    }
}
