//! A whole schema read at once: the details of its tables for the diagram and the linter,
//! their designs for the schema comparison.

use futures::{StreamExt, TryStreamExt};
use platypus_core::{Connection, ObjectKind, ObjectMeta, Result, TableDetails};
use platypus_sql::Dialect;
use serde::Serialize;

use crate::TableDesign;

/// How many tables are read at the same time.
const PARALLEL_READS: usize = 4;

/// A table of a schema with its details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaTable {
    pub name: String,
    pub comment: Option<String>,
    pub details: TableDetails,
    /// Why the details could not be read; they are empty then.
    pub error: Option<String>,
}

/// The tables of a schema, without the partitions of a table listed with them.
#[must_use]
pub fn tables_of(objects: &[ObjectMeta]) -> Vec<&ObjectMeta> {
    let is_table = |name: &str| {
        objects
            .iter()
            .any(|o| o.kind == ObjectKind::Table && o.name == name)
    };
    objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table && !o.parent.as_deref().is_some_and(is_table))
        .collect()
}

/// The tables of `schema` named in `only` (every table when `None`), in the order listed.
async fn chosen(
    conn: &dyn Connection,
    schema: &str,
    only: Option<&[String]>,
) -> Result<Vec<ObjectMeta>> {
    let objects = conn.objects(schema).await?;
    Ok(tables_of(&objects)
        .into_iter()
        .filter(|o| only.is_none_or(|names| names.contains(&o.name)))
        .cloned()
        .collect())
}

/// The details of the tables of `schema`, all or those in `only`. A table whose details
/// cannot be read is kept with the error, so one table does not hide the others.
pub async fn read_tables(
    conn: &dyn Connection,
    schema: &str,
    only: Option<&[String]>,
) -> Result<Vec<SchemaTable>> {
    let tables = chosen(conn, schema, only).await?;
    let read = futures::stream::iter(tables)
        .map(|o| async move {
            let read = conn.table_details(schema, &o.name).await;
            let error = read.as_ref().err().map(ToString::to_string);
            SchemaTable {
                name: o.name,
                comment: o.comment,
                details: read.unwrap_or_default(),
                error,
            }
        })
        .buffered(PARALLEL_READS)
        .collect()
        .await;
    Ok(read)
}

/// The designs of every table of `schema`, as [`crate::load`] reads them.
pub async fn load_designs(
    conn: &dyn Connection,
    dialect: Dialect,
    schema: &str,
) -> Result<Vec<TableDesign>> {
    let tables = chosen(conn, schema, None).await?;
    futures::stream::iter(tables)
        .map(|o| async move { crate::load(conn, dialect, schema, &o.name).await })
        .buffered(PARALLEL_READS)
        .try_collect()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(name: &str, kind: ObjectKind, parent: Option<&str>) -> ObjectMeta {
        ObjectMeta {
            name: name.to_owned(),
            kind,
            comment: None,
            parent: parent.map(str::to_owned),
            rows: None,
            size: None,
        }
    }

    #[test]
    fn partitions_and_views_are_not_tables_of_the_schema() {
        let objects = [
            object("events", ObjectKind::Table, None),
            object("events_2026", ObjectKind::Table, Some("events")),
            object("orphan", ObjectKind::Table, Some("gone")),
            object("recent", ObjectKind::View, None),
        ];
        let names: Vec<&str> = tables_of(&objects)
            .iter()
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(names, ["events", "orphan"]);
    }
}
