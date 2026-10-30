//! The navigator's view of a cluster, read from `system_schema` (which Cassandra 3 and later
//! and `ScyllaDB` share; the server-side `DESCRIBE` is newer and not everywhere): keyspaces are
//! the schemas, tables, materialized views, user-defined types, functions and aggregates the
//! objects, and a table's columns tell which are its partition and clustering keys.

use std::fmt::Write as _;

use platypus_core::{
    CATALOG_LIMIT, CatalogColumn, ColumnMeta, Connection, ConstraintKind, ConstraintMeta, Error,
    IndexMeta, ObjectKind, ObjectMeta, ObjectRef, Property, Result, SchemaMeta, TableDetails,
    TextRow, TextRowExt, query_text,
};
use serde_json::Value;

use crate::cql::{qualified, quote_ident, quote_string};

/// A keyspace that belongs to the server, not to the user's data.
#[must_use]
pub fn is_system(keyspace: &str) -> bool {
    keyspace.starts_with("system")
}

/// Keyspaces whose tables the server computes: Cassandra's virtual tables, which are listed
/// in a catalog of their own.
fn is_virtual(keyspace: &str) -> bool {
    matches!(keyspace, "system_views" | "system_virtual_schema")
}

/// The catalog keyspace that describes `keyspace`'s tables.
fn catalog(keyspace: &str) -> &'static str {
    if is_virtual(keyspace) {
        "system_virtual_schema"
    } else {
        "system_schema"
    }
}

/// `WHERE keyspace_name = 'ks'`, and the table when there is one.
fn scope(keyspace: &str, table: Option<(&str, &str)>) -> String {
    let mut out = format!("WHERE keyspace_name = {}", quote_string(keyspace));
    if let Some((column, name)) = table {
        let _ = write!(out, " AND {column} = {}", quote_string(name));
    }
    out
}

/// The elements of a JSON array of strings, as a list cell renders it.
fn list(cell: Option<String>) -> Vec<String> {
    cell.and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .unwrap_or_default()
}

/// The entries of a JSON object of strings, as a map cell renders it.
fn map(cell: Option<String>) -> Vec<(String, String)> {
    let Some(Value::Object(entries)) = cell.and_then(|text| serde_json::from_str(&text).ok())
    else {
        return Vec::new();
    };
    entries
        .into_iter()
        .map(|(k, v)| {
            let v = match v {
                Value::String(s) => s,
                other => other.to_string(),
            };
            (k, v)
        })
        .collect()
}

/// Runs a catalog query; a catalog the server lacks is no rows.
async fn rows(conn: &dyn Connection, cql: &str) -> Result<Vec<TextRow>> {
    query_text(conn, cql).await
}

/// The keyspaces, the system ones flagged; `current` is the one in use.
pub async fn schemas(conn: &dyn Connection, current: Option<&str>) -> Result<Vec<SchemaMeta>> {
    let mut names: Vec<String> = rows(conn, "SELECT keyspace_name FROM system_schema.keyspaces")
        .await?
        .iter()
        .map(|r| r.text(0))
        .collect();
    // Cassandra 4 lists its virtual keyspaces apart.
    if let Ok(virtual_rows) = rows(
        conn,
        "SELECT keyspace_name FROM system_virtual_schema.keyspaces",
    )
    .await
    {
        names.extend(virtual_rows.iter().map(|r| r.text(0)));
    }
    names.sort();
    names.dedup();
    Ok(names
        .into_iter()
        .map(|name| SchemaMeta {
            system: is_system(&name),
            current: current == Some(name.as_str()),
            name,
        })
        .collect())
}

/// What lives in `keyspace`.
pub async fn objects(conn: &dyn Connection, keyspace: &str) -> Result<Vec<ObjectMeta>> {
    let cat = catalog(keyspace);
    let where_ = scope(keyspace, None);
    let mut out = Vec::new();
    let comment = |cell: Option<String>| cell.filter(|c| !c.is_empty());
    let tables = format!("SELECT table_name, comment FROM {cat}.tables {where_}");
    for row in rows(conn, &tables).await? {
        out.push(ObjectMeta::new(
            row.text(0),
            ObjectKind::Table,
            comment(row.opt(1)),
        ));
    }
    if !is_virtual(keyspace) {
        let views =
            format!("SELECT view_name, base_table_name, comment FROM system_schema.views {where_}");
        for row in rows(conn, &views).await? {
            let mut view = ObjectMeta::new(
                row.text(0),
                ObjectKind::MaterializedView,
                comment(row.opt(2)),
            );
            view.parent = row.opt(1);
            out.push(view);
        }
        let types = format!("SELECT type_name FROM system_schema.types {where_}");
        for row in rows(conn, &types).await? {
            out.push(ObjectMeta::new(row.text(0), ObjectKind::Type, None));
        }
        // Functions and aggregates are optional on the servers that lack user code.
        let functions =
            format!("SELECT function_name, argument_types FROM system_schema.functions {where_}");
        for row in rows(conn, &functions).await.unwrap_or_default() {
            out.push(ObjectMeta::new(
                signature(&row.text(0), &list(row.opt(1))),
                ObjectKind::Function,
                None,
            ));
        }
        let aggregates =
            format!("SELECT aggregate_name, argument_types FROM system_schema.aggregates {where_}");
        for row in rows(conn, &aggregates).await.unwrap_or_default() {
            out.push(ObjectMeta::new(
                signature(&row.text(0), &list(row.opt(1))),
                ObjectKind::Function,
                Some("aggregate".to_owned()),
            ));
        }
    }
    out.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    Ok(out)
}

/// `name(int, text)`, how a function is named in the navigator.
fn signature(name: &str, args: &[String]) -> String {
    format!("{name}({})", args.join(", "))
}

/// A function's name and argument types from `name(int, text)`.
fn parse_signature(signature: &str) -> (String, Vec<String>) {
    let Some((name, rest)) = signature.split_once('(') else {
        return (signature.to_owned(), Vec::new());
    };
    let args = rest.trim_end().trim_end_matches(')');
    (
        name.trim().to_owned(),
        split_types(args).into_iter().collect(),
    )
}

/// Splits a type list at the commas that are not inside `<…>`.
fn split_types(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(text[start..i].trim().to_owned());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        out.push(last.to_owned());
    }
    out
}

/// A column of a table, with its role in the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub kind: Role,
    pub position: i64,
    pub descending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Partition,
    Clustering,
    Static,
    Regular,
}

/// The columns of a table or view in the order CQL lists them: the partition key, the
/// clustering columns, then the rest.
pub async fn columns(conn: &dyn Connection, keyspace: &str, table: &str) -> Result<Vec<Column>> {
    let cat = catalog(keyspace);
    let cql = format!(
        "SELECT column_name, kind, position, clustering_order, type FROM {cat}.columns {}",
        scope(keyspace, Some(("table_name", table)))
    );
    let mut columns: Vec<Column> = rows(conn, &cql)
        .await?
        .iter()
        .map(|r| Column {
            name: r.text(0),
            kind: match r.text(1).as_str() {
                "partition_key" => Role::Partition,
                "clustering" => Role::Clustering,
                "static" => Role::Static,
                _ => Role::Regular,
            },
            position: r.opt(2).and_then(|p| p.parse().ok()).unwrap_or(0),
            descending: r.text(3).eq_ignore_ascii_case("desc"),
            data_type: r.text(4),
        })
        .collect();
    if columns.is_empty() {
        return Err(Error::Other(format!(
            "there is no table or view {} in the cluster",
            qualified(keyspace, table)
        )));
    }
    columns.sort_by_key(|c| {
        let group = match c.kind {
            Role::Partition => 0,
            Role::Clustering => 1,
            Role::Static | Role::Regular => 2,
        };
        (group, c.position)
    });
    Ok(columns)
}

/// `(a, b), c, d` as the primary key clause writes the partition and clustering columns.
fn key_clause(columns: &[Column]) -> String {
    let partition: Vec<String> = columns
        .iter()
        .filter(|c| c.kind == Role::Partition)
        .map(|c| quote_ident(&c.name))
        .collect();
    let clustering = columns
        .iter()
        .filter(|c| c.kind == Role::Clustering)
        .map(|c| quote_ident(&c.name));
    let partition = if partition.len() == 1 {
        partition.join("")
    } else {
        format!("({})", partition.join(", "))
    };
    std::iter::once(partition)
        .chain(clustering)
        .collect::<Vec<_>>()
        .join(", ")
}

/// A column's role as the navigator's comment on it.
fn role_comment(column: &Column) -> Option<String> {
    match column.kind {
        Role::Partition => Some("partition key".to_owned()),
        Role::Clustering => Some(format!(
            "clustering key, {}",
            if column.descending {
                "descending"
            } else {
                "ascending"
            }
        )),
        Role::Static => Some("static".to_owned()),
        Role::Regular => None,
    }
}

/// The columns, key and secondary indexes of a table or view.
pub async fn table_details(
    conn: &dyn Connection,
    keyspace: &str,
    table: &str,
) -> Result<TableDetails> {
    let columns = columns(conn, keyspace, table).await?;
    let key: Vec<String> = columns
        .iter()
        .filter(|c| matches!(c.kind, Role::Partition | Role::Clustering))
        .map(|c| c.name.clone())
        .collect();
    let mut details = TableDetails {
        columns: columns
            .iter()
            .map(|c| ColumnMeta {
                name: c.name.clone(),
                data_type: c.data_type.clone(),
                nullable: matches!(c.kind, Role::Static | Role::Regular),
                default: None,
                primary_key: matches!(c.kind, Role::Partition | Role::Clustering),
                comment: role_comment(c),
                enum_values: Vec::new(),
                fields: Vec::new(),
            })
            .collect(),
        ..TableDetails::default()
    };
    if !key.is_empty() {
        details.indexes.push(IndexMeta {
            name: "PRIMARY KEY".to_owned(),
            columns: key.clone(),
            unique: true,
            primary: true,
            partial: false,
            size: None,
        });
        details.constraints.push(ConstraintMeta {
            name: "PRIMARY KEY".to_owned(),
            kind: ConstraintKind::PrimaryKey,
            columns: key,
            definition: format!("PRIMARY KEY ({})", key_clause(&columns)),
        });
    }
    if !is_virtual(keyspace) {
        details
            .indexes
            .extend(indexes(conn, keyspace, table).await?);
    }
    Ok(details)
}

/// The secondary indexes of a table.
async fn indexes(conn: &dyn Connection, keyspace: &str, table: &str) -> Result<Vec<IndexMeta>> {
    let cql = format!(
        "SELECT index_name, kind, options FROM system_schema.indexes {}",
        scope(keyspace, Some(("table_name", table)))
    );
    Ok(rows(conn, &cql)
        .await?
        .iter()
        .map(|r| {
            let options = map(r.opt(2));
            let target = options
                .iter()
                .find(|(k, _)| k == "target")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            IndexMeta {
                name: r.text(0),
                columns: vec![target],
                unique: false,
                primary: false,
                partial: false,
                size: None,
            }
        })
        .collect())
}

/// The user-defined types of `keyspace` with their fields, which the grid's edits need to
/// write a value of such a type.
pub async fn udts(conn: &dyn Connection, keyspace: &str) -> Result<crate::edit::Udts> {
    let cql = format!(
        "SELECT type_name, field_names, field_types FROM system_schema.types {}",
        scope(keyspace, None)
    );
    Ok(rows(conn, &cql)
        .await?
        .iter()
        .map(|r| {
            let fields = list(r.opt(1)).into_iter().zip(list(r.opt(2))).collect();
            (r.text(0), fields)
        })
        .collect())
}

/// Every column of every table, for completion.
pub async fn catalog_columns(conn: &dyn Connection) -> Result<Vec<CatalogColumn>> {
    let cql = "SELECT keyspace_name, table_name, column_name FROM system_schema.columns";
    Ok(rows(conn, cql)
        .await?
        .iter()
        .take(CATALOG_LIMIT)
        .map(|r| CatalogColumn {
            schema: r.text(0),
            table: r.text(1),
            column: r.text(2),
        })
        .collect())
}

/// Table options the DDL repeats, in the order `DESCRIBE` writes them.
const TABLE_OPTIONS: &[&str] = &[
    "bloom_filter_fp_chance",
    "caching",
    "comment",
    "compaction",
    "compression",
    "crc_check_chance",
    "default_time_to_live",
    "gc_grace_seconds",
    "max_index_interval",
    "memtable_flush_period_in_ms",
    "min_index_interval",
    "speculative_retry",
];

/// A row of a catalog table as `(column, text)` pairs.
async fn record(conn: &dyn Connection, cql: &str) -> Result<Option<Vec<(String, Option<String>)>>> {
    use futures::TryStreamExt as _;
    let platypus_core::ExecResult::Rows(stream) = conn
        .execute(cql, &platypus_core::ExecOptions::default())
        .await?
    else {
        return Ok(None);
    };
    let schema = stream.schema();
    let names: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
    let batches: Vec<_> = stream.try_collect().await?;
    let Some(batch) = batches.first().filter(|b| b.num_rows() > 0) else {
        return Ok(None);
    };
    let cells = batch
        .columns()
        .iter()
        .map(|c| arrow_cast_text(c, 0))
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(names.into_iter().zip(cells).collect()))
}

/// One cell of an array as text.
fn arrow_cast_text(array: &arrow_array::ArrayRef, row: usize) -> Result<Option<String>> {
    use arrow_array::Array as _;
    if array.is_null(row) {
        return Ok(None);
    }
    let formatter = arrow_cast::display::ArrayFormatter::try_new(
        array.as_ref(),
        &arrow_cast::display::FormatOptions::default(),
    )?;
    Ok(Some(formatter.value(row).to_string()))
}

/// The value of `column` in a record.
fn field<'a>(record: &'a [(String, Option<String>)], column: &str) -> Option<&'a str> {
    record
        .iter()
        .find(|(name, _)| name == column)
        .and_then(|(_, v)| v.as_deref())
}

/// `{'class': 'X', 'enabled': 'true'}` from a map cell.
fn map_literal(cell: &str) -> String {
    let entries = map(Some(cell.to_owned()));
    let body: Vec<String> = entries
        .iter()
        .map(|(k, v)| format!("{}: {}", quote_string(k), quote_string(v)))
        .collect();
    format!("{{{}}}", body.join(", "))
}

/// The `WITH` options of a table or view, one per line.
fn options_clause(record: &[(String, Option<String>)], ordering: &[Column]) -> Vec<String> {
    let mut options = Vec::new();
    let clustering: Vec<String> = ordering
        .iter()
        .filter(|c| c.kind == Role::Clustering)
        .map(|c| {
            format!(
                "{} {}",
                quote_ident(&c.name),
                if c.descending { "DESC" } else { "ASC" }
            )
        })
        .collect();
    if !clustering.is_empty() {
        options.push(format!("CLUSTERING ORDER BY ({})", clustering.join(", ")));
    }
    for name in TABLE_OPTIONS {
        let Some(value) = field(record, name) else {
            continue;
        };
        options.push(match *name {
            "comment" => {
                if value.is_empty() {
                    continue;
                }
                format!("comment = {}", quote_string(value))
            }
            "caching" | "compaction" | "compression" => {
                format!("{name} = {}", map_literal(value))
            }
            "speculative_retry" => format!("{name} = {}", quote_string(value)),
            _ => format!("{name} = {value}"),
        });
    }
    options
}

/// The statements that create a table or materialized view and its secondary indexes.
async fn relation_ddl(conn: &dyn Connection, object: &ObjectRef<'_>) -> Result<String> {
    let view = object.kind == ObjectKind::MaterializedView;
    let (catalog_table, name_column) = if view {
        ("views", "view_name")
    } else {
        ("tables", "table_name")
    };
    let cat = catalog(object.schema);
    let cql = format!(
        "SELECT * FROM {cat}.{catalog_table} {}",
        scope(object.schema, Some((name_column, object.name)))
    );
    let Some(record) = record(conn, &cql).await? else {
        return Err(Error::Other(format!(
            "there is no {} {}",
            if view { "materialized view" } else { "table" },
            qualified(object.schema, object.name)
        )));
    };
    let columns = columns(conn, object.schema, object.name).await?;
    let target = qualified(object.schema, object.name);
    let mut out = String::new();
    if view {
        let base = field(&record, "base_table_name").unwrap_or_default();
        let selected = if field(&record, "include_all_columns") == Some("true") {
            "*".to_owned()
        } else {
            columns
                .iter()
                .map(|c| quote_ident(&c.name))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = write!(
            out,
            "CREATE MATERIALIZED VIEW {target} AS\n    SELECT {selected}\n    FROM {}",
            qualified(object.schema, base)
        );
        if let Some(filter) = field(&record, "where_clause").filter(|w| !w.is_empty()) {
            let _ = write!(out, "\n    WHERE {filter}");
        }
        let _ = write!(out, "\n    PRIMARY KEY ({})", key_clause(&columns));
    } else {
        let _ = writeln!(out, "CREATE TABLE {target} (");
        for column in &columns {
            let is_static = if column.kind == Role::Static {
                " STATIC"
            } else {
                ""
            };
            let _ = writeln!(
                out,
                "    {} {}{is_static},",
                quote_ident(&column.name),
                column.data_type
            );
        }
        let _ = write!(out, "    PRIMARY KEY ({})\n)", key_clause(&columns));
    }
    let options = options_clause(&record, &columns);
    for (i, option) in options.iter().enumerate() {
        out.push_str(if i == 0 { " WITH " } else { "\n    AND " });
        out.push_str(option);
    }
    out.push_str(";\n");
    if !view {
        out.push_str(&index_ddl(conn, object).await?);
    }
    Ok(out)
}

/// `CREATE INDEX` for each secondary index of the table.
async fn index_ddl(conn: &dyn Connection, object: &ObjectRef<'_>) -> Result<String> {
    let cql = format!(
        "SELECT index_name, kind, options FROM system_schema.indexes {}",
        scope(object.schema, Some(("table_name", object.name)))
    );
    let mut out = String::new();
    for row in rows(conn, &cql).await? {
        let options = map(row.opt(2));
        let target = options
            .iter()
            .find(|(k, _)| k == "target")
            .map_or("", |(_, v)| v.as_str());
        let table = qualified(object.schema, object.name);
        let name = quote_ident(&row.text(0));
        if row.text(1) == "CUSTOM" {
            let class = options
                .iter()
                .find(|(k, _)| k == "class_name")
                .map_or("", |(_, v)| v.as_str());
            let rest: Vec<String> = options
                .iter()
                .filter(|(k, _)| k != "target" && k != "class_name")
                .map(|(k, v)| format!("{}: {}", quote_string(k), quote_string(v)))
                .collect();
            let _ = write!(
                out,
                "\nCREATE CUSTOM INDEX {name} ON {table} ({target}) USING {}",
                quote_string(class)
            );
            if !rest.is_empty() {
                let _ = write!(out, " WITH OPTIONS = {{{}}}", rest.join(", "));
            }
            out.push_str(";\n");
        } else {
            let _ = write!(out, "\nCREATE INDEX {name} ON {table} ({target});\n");
        }
    }
    Ok(out)
}

/// `CREATE TYPE` for a user-defined type.
async fn type_ddl(conn: &dyn Connection, object: &ObjectRef<'_>) -> Result<String> {
    let cql = format!(
        "SELECT field_names, field_types FROM system_schema.types {}",
        scope(object.schema, Some(("type_name", object.name)))
    );
    let Some(row) = rows(conn, &cql).await?.into_iter().next() else {
        return Err(Error::Other(format!(
            "there is no type {}",
            qualified(object.schema, object.name)
        )));
    };
    let names = list(row.opt(0));
    let types = list(row.opt(1));
    let fields: Vec<String> = names
        .iter()
        .zip(&types)
        .map(|(n, t)| format!("    {} {t}", quote_ident(n)))
        .collect();
    Ok(format!(
        "CREATE TYPE {} (\n{}\n);\n",
        qualified(object.schema, object.name),
        fields.join(",\n")
    ))
}

/// `CREATE FUNCTION` or `CREATE AGGREGATE` for the function the signature names.
async fn function_ddl(conn: &dyn Connection, object: &ObjectRef<'_>) -> Result<String> {
    let (name, args) = parse_signature(object.name);
    let target = qualified(object.schema, &name);
    let cql = format!(
        "SELECT argument_names, argument_types, called_on_null_input, language, return_type, body \
         FROM system_schema.functions {}",
        scope(object.schema, Some(("function_name", &name)))
    );
    for row in rows(conn, &cql).await.unwrap_or_default() {
        if list(row.opt(1)) != args {
            continue;
        }
        let params: Vec<String> = list(row.opt(0))
            .iter()
            .zip(&args)
            .map(|(n, t)| format!("{} {t}", quote_ident(n)))
            .collect();
        let null_input = if row.flag(2) {
            "CALLED ON NULL INPUT"
        } else {
            "RETURNS NULL ON NULL INPUT"
        };
        return Ok(format!(
            "CREATE OR REPLACE FUNCTION {target} ({})\n    {null_input}\n    RETURNS {}\n    LANGUAGE {}\n    AS $${}$$;\n",
            params.join(", "),
            row.text(4),
            row.text(3),
            row.text(5)
        ));
    }
    let cql = format!(
        "SELECT argument_types, state_func, state_type, final_func, initcond \
         FROM system_schema.aggregates {}",
        scope(object.schema, Some(("aggregate_name", &name)))
    );
    for row in rows(conn, &cql).await.unwrap_or_default() {
        if list(row.opt(0)) != args {
            continue;
        }
        let mut out = format!(
            "CREATE OR REPLACE AGGREGATE {target} ({})\n    SFUNC {}\n    STYPE {}",
            args.join(", "),
            quote_ident(&row.text(1)),
            row.text(2)
        );
        if let Some(finalfunc) = row.opt(3) {
            let _ = write!(out, "\n    FINALFUNC {}", quote_ident(&finalfunc));
        }
        if let Some(init) = row.opt(4) {
            let _ = write!(out, "\n    INITCOND {init}");
        }
        out.push_str(";\n");
        return Ok(out);
    }
    Err(Error::Other(format!(
        "there is no function {}",
        qualified(object.schema, object.name)
    )))
}

/// The statements that create `object`.
pub async fn object_ddl(conn: &dyn Connection, object: &ObjectRef<'_>) -> Result<String> {
    match object.kind {
        ObjectKind::Table | ObjectKind::MaterializedView => relation_ddl(conn, object).await,
        ObjectKind::Type => type_ddl(conn, object).await,
        ObjectKind::Function => function_ddl(conn, object).await,
        other => Err(platypus_core::unsupported(&format!(
            "the DDL of a {other:?} in Cassandra"
        ))),
    }
}

/// The options and keys of a table, a view or a type, one line each.
pub async fn object_properties(
    conn: &dyn Connection,
    object: &ObjectRef<'_>,
) -> Result<Vec<Property>> {
    let mut out = vec![Property::new("Keyspace", object.schema)];
    match object.kind {
        ObjectKind::Table | ObjectKind::MaterializedView => {
            let columns = columns(conn, object.schema, object.name).await?;
            let names = |role: Role| {
                columns
                    .iter()
                    .filter(|c| c.kind == role)
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            out.push(Property::new("Partition key", names(Role::Partition)));
            let clustering = names(Role::Clustering);
            if !clustering.is_empty() {
                out.push(Property::new("Clustering key", clustering));
            }
            let (catalog_table, name_column) = if object.kind == ObjectKind::Table {
                ("tables", "table_name")
            } else {
                ("views", "view_name")
            };
            let cql = format!(
                "SELECT * FROM {}.{catalog_table} {}",
                catalog(object.schema),
                scope(object.schema, Some((name_column, object.name)))
            );
            if let Some(record) = record(conn, &cql).await? {
                for name in TABLE_OPTIONS {
                    if let Some(value) = field(&record, name).filter(|v| !v.is_empty()) {
                        out.push(Property::new(*name, value));
                    }
                }
            }
        }
        ObjectKind::Type => {
            let ddl = type_ddl(conn, object).await?;
            out.push(Property::new(
                "Fields",
                ddl.lines().count().saturating_sub(2).to_string(),
            ));
        }
        _ => {}
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, kind: Role, position: i64) -> Column {
        Column {
            name: name.to_owned(),
            data_type: "int".to_owned(),
            kind,
            position,
            descending: false,
        }
    }

    #[test]
    fn writes_the_primary_key_clause() {
        let one = [
            column("id", Role::Partition, 0),
            column("at", Role::Clustering, 0),
        ];
        assert_eq!(key_clause(&one), "id, at");
        let composite = [
            column("a", Role::Partition, 0),
            column("B", Role::Partition, 1),
            column("c", Role::Clustering, 0),
        ];
        assert_eq!(key_clause(&composite), "(a, \"B\"), c");
    }

    #[test]
    fn reads_cells_the_driver_renders() {
        assert_eq!(list(Some(r#"["int","text"]"#.into())), ["int", "text"]);
        assert_eq!(list(None), Vec::<String>::new());
        assert_eq!(
            map(Some(r#"{"class":"A","n":"1"}"#.into())),
            [("class".into(), "A".into()), ("n".into(), "1".into())]
        );
        assert_eq!(map_literal(r#"{"class":"it's"}"#), "{'class': 'it''s'}");
    }

    #[test]
    fn splits_function_signatures() {
        assert_eq!(
            parse_signature("f(int, map<text, list<int>>)"),
            (
                "f".to_owned(),
                vec!["int".to_owned(), "map<text, list<int>>".to_owned()]
            )
        );
        assert_eq!(parse_signature("g()"), ("g".to_owned(), Vec::new()));
        assert_eq!(signature("f", &["int".to_owned()]), "f(int)");
    }

    #[test]
    fn knows_system_keyspaces() {
        assert!(is_system("system_schema"));
        assert!(!is_system("shop"));
        assert_eq!(scope("sh'op", None), "WHERE keyspace_name = 'sh''op'");
    }
}
