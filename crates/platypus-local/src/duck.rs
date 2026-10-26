//! A DuckDB database filled with result rows, to query them with SQL.

use std::collections::HashSet;
use std::sync::Arc;

use arrow_array::{ArrayRef, RecordBatch, StringArray};
use arrow_cast::display::FormatOptions;
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use duckdb::vtab::arrow::{ArrowBatchRegistration, ArrowVTab};

use crate::{LocalError, values};

/// The table function that reads a registered Arrow batch.
const SCAN: &str = "platypus_arrow";

impl From<duckdb::Error> for LocalError {
    fn from(e: duckdb::Error) -> Self {
        Self::Duck(e.to_string())
    }
}

/// Creates `table` in `conn`, replacing one of that name, with the rows of `batches`; returns
/// how many. Columns DuckDB cannot store become text; repeated or empty names get new ones.
pub fn load(
    conn: &duckdb::Connection,
    table: &str,
    schema: &SchemaRef,
    batches: &[RecordBatch],
) -> Result<usize, LocalError> {
    register(conn)?;
    let (schema, batches) = storable(schema, batches)?;
    let name = quote(table);
    conn.execute_batch(&format!("DROP TABLE IF EXISTS {name}"))?;
    let empty = ArrowBatchRegistration::new(RecordBatch::new_empty(schema));
    conn.execute(
        &format!("CREATE TABLE {name} AS SELECT * FROM {SCAN}(?)"),
        [&empty],
    )?;
    let mut insert = conn.prepare(&format!("INSERT INTO {name} SELECT * FROM {SCAN}(?)"))?;
    let mut rows = 0;
    for batch in batches {
        rows += batch.num_rows();
        let registration = ArrowBatchRegistration::new(batch);
        insert.execute([&registration])?;
    }
    Ok(rows)
}

/// Drops `table` from `conn` if it is there.
pub fn drop_table(conn: &duckdb::Connection, table: &str) -> Result<(), LocalError> {
    conn.execute_batch(&format!("DROP TABLE IF EXISTS {}", quote(table)))?;
    Ok(())
}

/// `name` as a DuckDB identifier.
#[must_use]
pub fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Column names DuckDB accepts in one table: never empty, distinct ignoring case, the later
/// ones of a name numbered from 2.
#[must_use]
pub fn unique_names(names: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let base = if name.is_empty() {
                format!("column{}", i + 1)
            } else {
                name.clone()
            };
            let mut candidate = base.clone();
            let mut n = 2;
            while !seen.insert(candidate.to_lowercase()) {
                candidate = format!("{base}_{n}");
                n += 1;
            }
            candidate
        })
        .collect()
}

/// Registers the Arrow scan once per database: its connections share it.
fn register(conn: &duckdb::Connection) -> Result<(), LocalError> {
    let present: i64 = conn.query_row(
        "SELECT count(*) FROM duckdb_functions() WHERE function_name = ?",
        [SCAN],
        |r| r.get(0),
    )?;
    if present == 0 {
        conn.register_table_function::<ArrowVTab>(SCAN)?;
    }
    Ok(())
}

/// What DuckDB can store: names made unique, text in place of the types it cannot take.
fn storable(
    schema: &SchemaRef,
    batches: &[RecordBatch],
) -> Result<(SchemaRef, Vec<RecordBatch>), LocalError> {
    let names: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
    let text: Vec<bool> = schema.fields().iter().map(|f| !takes(f)).collect();
    let fields: Vec<Field> = schema
        .fields()
        .iter()
        .zip(unique_names(&names))
        .zip(&text)
        .map(|((f, name), &text)| {
            let data_type = if text {
                DataType::Utf8
            } else {
                f.data_type().clone()
            };
            Field::new(name, data_type, true)
        })
        .collect();
    let schema = Arc::new(Schema::new(fields));
    let batches = batches
        .iter()
        .map(|batch| {
            let columns = batch
                .columns()
                .iter()
                .zip(&text)
                .map(|(c, &text)| if text { as_text(c) } else { Ok(Arc::clone(c)) })
                .collect::<Result<Vec<_>, LocalError>>()?;
            Ok(RecordBatch::try_new(Arc::clone(&schema), columns)?)
        })
        .collect::<Result<Vec<_>, LocalError>>()?;
    Ok((schema, batches))
}

/// DuckDB stores the column as it is. `Decimal256` binds but cannot be written, dictionaries
/// and half floats have no conversion.
fn takes(field: &Field) -> bool {
    !matches!(
        field.data_type(),
        DataType::Decimal256(_, _) | DataType::Dictionary(_, _) | DataType::Float16
    ) && duckdb::vtab::to_duckdb_logical_type_for_field(field).is_ok()
}

/// The values as the text the grid shows.
fn as_text(array: &ArrayRef) -> Result<ArrayRef, LocalError> {
    let texts = values::texts(array.as_ref(), &FormatOptions::default())?;
    Ok(Arc::new(StringArray::from(texts)))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Int32Array, NullArray, RecordBatch, StringArray};
    use arrow_schema::{DataType, Field, Schema, SchemaRef};

    use super::*;

    fn people(ids: &[i32]) -> (SchemaRef, RecordBatch) {
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, true),
        ]));
        let names: Vec<String> = ids.iter().map(|i| format!("p{i}")).collect();
        let columns: Vec<ArrayRef> = vec![
            Arc::new(Int32Array::from(ids.to_vec())),
            Arc::new(StringArray::from(names)),
        ];
        let batch = RecordBatch::try_new(Arc::clone(&schema), columns).unwrap();
        (schema, batch)
    }

    fn one(conn: &duckdb::Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn loads_batches_as_a_table() {
        let conn = duckdb::Connection::open_in_memory().unwrap();
        let (schema, first) = people(&[1, 2]);
        let (_, second) = people(&[3]);
        assert_eq!(load(&conn, "result", &schema, &[first, second]).unwrap(), 3);
        assert_eq!(one(&conn, "SELECT count(*) FROM result"), 3);
        assert_eq!(
            one(
                &conn,
                "SELECT sum(id)::BIGINT FROM result WHERE name = 'p2'"
            ),
            2
        );
    }

    #[test]
    fn loading_again_replaces_the_table() {
        let conn = duckdb::Connection::open_in_memory().unwrap();
        let (schema, batch) = people(&[1, 2, 3]);
        load(&conn, "result", &schema, &[batch]).unwrap();
        let (_, batch) = people(&[9]);
        load(&conn, "result", &schema, &[batch]).unwrap();
        assert_eq!(one(&conn, "SELECT count(*) FROM result"), 1);
        drop_table(&conn, "result").unwrap();
        assert_eq!(
            one(
                &conn,
                "SELECT count(*) FROM duckdb_tables() WHERE table_name = 'result'"
            ),
            0
        );
    }

    #[test]
    fn an_empty_result_is_an_empty_table() {
        let conn = duckdb::Connection::open_in_memory().unwrap();
        let (schema, _) = people(&[]);
        assert_eq!(load(&conn, "r1", &schema, &[]).unwrap(), 0);
        assert_eq!(one(&conn, "SELECT count(*) FROM r1"), 0);
    }

    #[test]
    fn names_and_types_duckdb_cannot_take() {
        let conn = duckdb::Connection::open_in_memory().unwrap();
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("ID", DataType::Int32, false),
            Field::new("", DataType::Null, true),
        ]));
        let columns: Vec<ArrayRef> = vec![
            Arc::new(Int32Array::from(vec![1])),
            Arc::new(Int32Array::from(vec![2])),
            Arc::new(NullArray::new(1)),
        ];
        let batch = RecordBatch::try_new(Arc::clone(&schema), columns).unwrap();
        load(&conn, "odd", &schema, &[batch]).unwrap();
        let mut stmt = conn
            .prepare("SELECT column_name, data_type FROM duckdb_columns() WHERE table_name = 'odd' ORDER BY column_index")
            .unwrap();
        let columns: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let expected = [
            ("id", "INTEGER"),
            ("ID_2", "INTEGER"),
            ("column3", "VARCHAR"),
        ]
        .map(|(n, t)| (n.to_owned(), t.to_owned()));
        assert_eq!(columns, expected);
    }

    #[test]
    fn unique_names_ignore_case() {
        let names = ["a", "A", "a", ""].map(String::from);
        assert_eq!(unique_names(&names), ["a", "A_2", "a_3", "column4"]);
        assert_eq!(quote("we\"ird"), "\"we\"\"ird\"");
    }
}
