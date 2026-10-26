//! The result of ADBC's `GetObjects`, a nested table (catalogs › schemas › tables › columns
//! and constraints), read into plain structs, then into the app's metadata.

use arrow_array::cast::AsArray as _;
use arrow_array::{Array, ArrayRef, RecordBatch, StructArray};
use arrow_schema::DataType;
use platypus_core::{
    CatalogColumn, ColumnMeta, Error, ForeignKeyMeta, IndexMeta, ObjectKind, ObjectMeta, Result,
    SchemaMeta, TableDetails,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub name: String,
    pub schemas: Vec<SchemaNode>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaNode {
    pub name: String,
    pub tables: Vec<TableNode>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableNode {
    pub name: String,
    /// `TABLE`, `VIEW`, `MATERIALIZED VIEW`…, as the driver names it.
    pub kind: String,
    pub columns: Vec<ColumnNode>,
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnNode {
    pub name: String,
    pub type_name: Option<String>,
    pub nullable: Option<bool>,
    pub default: Option<String>,
    pub remarks: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Constraint {
    pub name: Option<String>,
    /// `PRIMARY KEY`, `FOREIGN KEY`, `UNIQUE`, `CHECK`.
    pub kind: String,
    pub columns: Vec<String>,
    /// For a foreign key, the referenced `(schema, table, column)` of each column.
    pub references: Vec<(Option<String>, String, String)>,
}

fn malformed(what: &str) -> Error {
    Error::Unsupported(format!("unexpected GetObjects result: {what}"))
}

/// A child column of a struct, by name; `None` when the driver leaves it out.
fn field<'a>(s: &'a StructArray, name: &str) -> Option<&'a ArrayRef> {
    s.column_by_name(name)
}

/// Row `i` of a text column, whatever string type the driver used.
fn text(array: Option<&ArrayRef>, i: usize) -> Option<String> {
    let array = array?;
    if array.is_null(i) {
        return None;
    }
    if array.data_type() == &DataType::Utf8 {
        return Some(array.as_string::<i32>().value(i).to_owned());
    }
    // Only the row is cast, not the whole column for each row.
    let cast = arrow_cast::cast(&array.slice(i, 1), &DataType::Utf8).ok()?;
    Some(cast.as_string::<i32>().value(0).to_owned())
}

fn int(array: Option<&ArrayRef>, i: usize) -> Option<i64> {
    let array = array?;
    if array.is_null(i) {
        return None;
    }
    let cast = arrow_cast::cast(&array.slice(i, 1), &DataType::Int64).ok()?;
    Some(
        cast.as_primitive::<arrow_array::types::Int64Type>()
            .value(0),
    )
}

/// The elements of row `i` of a list column; `None` for null or missing.
fn list_value(array: Option<&ArrayRef>, i: usize) -> Option<ArrayRef> {
    let array = array.filter(|a| !a.is_null(i))?;
    Some(array.as_list_opt::<i32>()?.value(i))
}

/// The struct elements of row `i` of a list column.
fn list_items(array: Option<&ArrayRef>, i: usize) -> Option<StructArray> {
    list_value(array, i)?.as_struct_opt().cloned()
}

fn strings(array: Option<&ArrayRef>, i: usize) -> Vec<String> {
    list_value(array, i)
        .map(|values| {
            (0..values.len())
                .filter_map(|j| text(Some(&values), j))
                .collect()
        })
        .unwrap_or_default()
}

/// Reads the batches of `GetObjects`.
pub fn parse_objects(batches: &[RecordBatch]) -> Result<Vec<Catalog>> {
    let mut out = Vec::new();
    for batch in batches {
        let names = batch
            .column_by_name("catalog_name")
            .ok_or_else(|| malformed("no catalog_name"))?;
        let schemas = batch.column_by_name("catalog_db_schemas");
        for i in 0..batch.num_rows() {
            out.push(Catalog {
                name: text(Some(names), i).unwrap_or_default(),
                schemas: list_items(schemas, i)
                    .map(|s| parse_schemas(&s))
                    .unwrap_or_default(),
            });
        }
    }
    Ok(out)
}

fn parse_schemas(s: &StructArray) -> Vec<SchemaNode> {
    (0..s.len())
        .map(|i| SchemaNode {
            name: text(field(s, "db_schema_name"), i).unwrap_or_default(),
            tables: list_items(field(s, "db_schema_tables"), i)
                .map(|t| parse_tables(&t))
                .unwrap_or_default(),
        })
        .collect()
}

fn parse_tables(t: &StructArray) -> Vec<TableNode> {
    (0..t.len())
        .map(|i| TableNode {
            name: text(field(t, "table_name"), i).unwrap_or_default(),
            kind: text(field(t, "table_type"), i).unwrap_or_default(),
            columns: list_items(field(t, "table_columns"), i)
                .map(|c| parse_columns(&c))
                .unwrap_or_default(),
            constraints: list_items(field(t, "table_constraints"), i)
                .map(|c| parse_constraints(&c))
                .unwrap_or_default(),
        })
        .collect()
}

fn parse_columns(c: &StructArray) -> Vec<ColumnNode> {
    let mut columns: Vec<(i64, ColumnNode)> = (0..c.len())
        .map(|i| {
            let position = int(field(c, "ordinal_position"), i).unwrap_or(i64::MAX);
            let column = ColumnNode {
                name: text(field(c, "column_name"), i).unwrap_or_default(),
                type_name: text(field(c, "xdbc_type_name"), i),
                // 0: no nulls, 1: nullable (ODBC's SQL_NULLABLE).
                nullable: int(field(c, "xdbc_nullable"), i).map(|n| n != 0),
                default: text(field(c, "xdbc_column_def"), i),
                remarks: text(field(c, "remarks"), i).filter(|r| !r.is_empty()),
            };
            (position, column)
        })
        .collect();
    columns.sort_by_key(|(position, _)| *position);
    columns.into_iter().map(|(_, c)| c).collect()
}

fn parse_constraints(c: &StructArray) -> Vec<Constraint> {
    (0..c.len())
        .map(|i| {
            let references = list_items(field(c, "constraint_column_usage"), i)
                .map(|u| {
                    (0..u.len())
                        .map(|j| {
                            (
                                text(field(&u, "fk_db_schema"), j),
                                text(field(&u, "fk_table"), j).unwrap_or_default(),
                                text(field(&u, "fk_column_name"), j).unwrap_or_default(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            Constraint {
                name: text(field(c, "constraint_name"), i),
                kind: text(field(c, "constraint_type"), i)
                    .unwrap_or_default()
                    .to_ascii_uppercase(),
                columns: strings(field(c, "constraint_column_names"), i),
                references,
            }
        })
        .collect()
}

/// Every table of the catalogs read.
fn tables(catalogs: &[Catalog]) -> impl Iterator<Item = &TableNode> {
    catalogs
        .iter()
        .flat_map(|c| &c.schemas)
        .flat_map(|s| &s.tables)
}

/// Schemas only the database uses.
fn is_system(name: &str) -> bool {
    name.eq_ignore_ascii_case("information_schema")
        || name.eq_ignore_ascii_case("sys")
        || name.eq_ignore_ascii_case("pg_catalog")
}

/// The schemas of the catalogs read, `current` marked.
pub fn schemas(catalogs: &[Catalog], current: Option<&str>) -> Vec<SchemaMeta> {
    let mut out: Vec<SchemaMeta> = catalogs
        .iter()
        .flat_map(|c| &c.schemas)
        .map(|s| SchemaMeta {
            name: s.name.clone(),
            system: is_system(&s.name),
            current: current.is_some_and(|c| c.eq_ignore_ascii_case(&s.name)),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out
}

fn kind(table_type: &str) -> ObjectKind {
    let upper = table_type.to_ascii_uppercase();
    if upper.contains("MATERIALIZED") {
        ObjectKind::MaterializedView
    } else if upper.contains("VIEW") {
        ObjectKind::View
    } else {
        ObjectKind::Table
    }
}

/// The tables and views of the catalogs read, by name.
pub fn objects(catalogs: &[Catalog]) -> Vec<ObjectMeta> {
    let mut out: Vec<ObjectMeta> = tables(catalogs)
        .map(|t| ObjectMeta::new(&t.name, kind(&t.kind), None))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The columns, primary key and foreign keys of the one table read.
pub fn table_details(catalogs: &[Catalog]) -> TableDetails {
    let Some(table) = tables(catalogs).next() else {
        return TableDetails::default();
    };
    let primary = table.constraints.iter().find(|c| c.kind == "PRIMARY KEY");
    let in_key = |name: &str| primary.is_some_and(|p| p.columns.iter().any(|c| c == name));
    let columns = table
        .columns
        .iter()
        .map(|c| ColumnMeta {
            name: c.name.clone(),
            data_type: c.type_name.clone().unwrap_or_default(),
            nullable: c.nullable.unwrap_or(true),
            default: c.default.clone(),
            primary_key: in_key(&c.name),
            comment: c.remarks.clone(),
            enum_values: Vec::new(),
            fields: Vec::new(),
        })
        .collect();
    let indexes = primary
        .map(|p| IndexMeta {
            name: p.name.clone().unwrap_or_else(|| "PRIMARY KEY".to_owned()),
            columns: p.columns.clone(),
            unique: true,
            primary: true,
            partial: false,
            size: None,
        })
        .into_iter()
        .collect();
    let foreign_keys = table
        .constraints
        .iter()
        .filter(|c| c.kind == "FOREIGN KEY")
        .filter_map(|c| {
            let (schema, table, _) = c.references.first()?;
            Some(ForeignKeyMeta {
                name: c.name.clone().unwrap_or_default(),
                columns: c.columns.clone(),
                ref_schema: schema.clone().unwrap_or_default(),
                ref_table: table.clone(),
                ref_columns: c.references.iter().map(|r| r.2.clone()).collect(),
            })
        })
        .collect();
    TableDetails {
        columns,
        indexes,
        foreign_keys,
        ..TableDetails::default()
    }
}

/// Every column of every table read, for autocompletion; at most `limit`.
pub fn catalog_columns(catalogs: &[Catalog], limit: usize) -> Vec<CatalogColumn> {
    catalogs
        .iter()
        .flat_map(|c| &c.schemas)
        .filter(|s| !is_system(&s.name))
        .flat_map(|s| {
            s.tables.iter().flat_map(move |t| {
                t.columns.iter().map(move |c| CatalogColumn {
                    schema: s.name.clone(),
                    table: t.name.clone(),
                    column: c.name.clone(),
                })
            })
        })
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_schema::{DataType as T, Field, Fields, Schema};

    use super::*;

    fn list(item: DataType) -> DataType {
        T::List(Arc::new(Field::new("item", item, true)))
    }

    /// The schema of `GetObjects` from the ADBC specification.
    fn schema() -> Schema {
        let usage = T::Struct(Fields::from(vec![
            Field::new("fk_catalog", T::Utf8, true),
            Field::new("fk_db_schema", T::Utf8, true),
            Field::new("fk_table", T::Utf8, false),
            Field::new("fk_column_name", T::Utf8, false),
        ]));
        let constraint = T::Struct(Fields::from(vec![
            Field::new("constraint_name", T::Utf8, true),
            Field::new("constraint_type", T::Utf8, false),
            Field::new("constraint_column_names", list(T::Utf8), false),
            Field::new("constraint_column_usage", list(usage), true),
        ]));
        let column = T::Struct(Fields::from(vec![
            Field::new("column_name", T::Utf8, false),
            Field::new("ordinal_position", T::Int32, true),
            Field::new("remarks", T::Utf8, true),
            Field::new("xdbc_type_name", T::Utf8, true),
            Field::new("xdbc_nullable", T::Int16, true),
            Field::new("xdbc_column_def", T::Utf8, true),
        ]));
        let table = T::Struct(Fields::from(vec![
            Field::new("table_name", T::Utf8, false),
            Field::new("table_type", T::Utf8, false),
            Field::new("table_columns", list(column), true),
            Field::new("table_constraints", list(constraint), true),
        ]));
        let db_schema = T::Struct(Fields::from(vec![
            Field::new("db_schema_name", T::Utf8, true),
            Field::new("db_schema_tables", list(table), true),
        ]));
        Schema::new(vec![
            Field::new("catalog_name", T::Utf8, true),
            Field::new("catalog_db_schemas", list(db_schema), true),
        ])
    }

    fn batch(json: &str) -> RecordBatch {
        let mut reader = arrow_json::ReaderBuilder::new(Arc::new(schema()))
            .build(std::io::Cursor::new(json))
            .unwrap();
        reader.next().unwrap().unwrap()
    }

    const OBJECTS: &str = r#"{"catalog_name": "memory", "catalog_db_schemas": [
        {"db_schema_name": "information_schema", "db_schema_tables": []},
        {"db_schema_name": "shop", "db_schema_tables": [
          {"table_name": "orders", "table_type": "TABLE",
           "table_columns": [
             {"column_name": "customer", "ordinal_position": 2, "xdbc_type_name": "bigint", "xdbc_nullable": 1},
             {"column_name": "id", "ordinal_position": 1, "xdbc_type_name": "bigint", "xdbc_nullable": 0, "remarks": "key"}],
           "table_constraints": [
             {"constraint_name": "pk", "constraint_type": "PRIMARY KEY", "constraint_column_names": ["id"]},
             {"constraint_name": "fk", "constraint_type": "FOREIGN KEY", "constraint_column_names": ["customer"],
              "constraint_column_usage": [{"fk_db_schema": "shop", "fk_table": "customers", "fk_column_name": "id"}]}]},
          {"table_name": "totals", "table_type": "VIEW", "table_columns": null, "table_constraints": null}]}]}"#;

    #[test]
    fn parses_get_objects() {
        let catalogs = parse_objects(&[batch(OBJECTS)]).unwrap();
        assert_eq!(catalogs.len(), 1);
        let s = schemas(&catalogs, Some("SHOP"));
        assert_eq!(
            s.iter()
                .map(|s| (s.name.as_str(), s.system, s.current))
                .collect::<Vec<_>>(),
            [("information_schema", true, false), ("shop", false, true)]
        );
        let o = objects(&catalogs);
        assert_eq!(
            o.iter()
                .map(|o| (o.name.as_str(), o.kind))
                .collect::<Vec<_>>(),
            [("orders", ObjectKind::Table), ("totals", ObjectKind::View)]
        );
        let d = table_details(&catalogs);
        // In ordinal order, not the driver's.
        assert_eq!(d.columns[0].name, "id");
        assert!(d.columns[0].primary_key && !d.columns[0].nullable);
        assert_eq!(d.columns[0].comment.as_deref(), Some("key"));
        assert!(d.columns[1].nullable);
        assert_eq!(d.indexes[0].columns, ["id"]);
        assert_eq!(d.foreign_keys[0].ref_table, "customers");
        assert_eq!(d.foreign_keys[0].ref_columns, ["id"]);
        let c = catalog_columns(&catalogs, 10);
        assert_eq!(c.len(), 2);
        assert_eq!(catalog_columns(&catalogs, 1).len(), 1);
    }
}
