//! Navigator metadata from DuckDB's `duckdb_*()` catalog functions: the schemas of the
//! current database (the opened file), and each attached database as a schema of its name,
//! its `main` schema, which SQL names the same way (`other.t`).

use platypus_core::{
    CATALOG_LIMIT, CatalogColumn, CatalogForeignKey, ColumnMeta, Connection, ConstraintKind,
    ConstraintMeta, Error, ForeignKeyMeta, IndexMeta, MatchKind, MetadataMatch, ObjectKind,
    ObjectMeta, ObjectRef, Property, ReferencingKey, Result, SEARCH_LIMIT, SchemaMeta,
    TableDetails, TextRow, TextRowExt, query_catalog, query_text, unsupported,
};
use platypus_sql::Dialect;

const D: Dialect = Dialect::DuckDb;

/// Separates the items of a list column turned into text with `array_to_string`.
const SEP: char = '\u{1f}';

/// The rows of the current database, in the catalog function's own columns.
const HERE: &str = "database_name = current_database()";

/// The rows of the schema the navigator lists as `s` (a literal), in the catalog function's
/// own columns: a schema of the current database, or else the `main` schema of the attached
/// database of that name. `same` compares two names.
fn listed_as(s: &str, same: fn(&str, &str) -> String) -> String {
    format!(
        "(({HERE} AND {}) OR ({} AND schema_name = 'main' AND NOT EXISTS (
             SELECT 1 FROM duckdb_schemas() x WHERE x.{HERE} AND {})))",
        same("schema_name", s),
        same("database_name", s),
        same("x.schema_name", s),
    )
}

fn equal(a: &str, b: &str) -> String {
    format!("{a} = {b}")
}

/// DuckDB names are case-insensitive, quoted or not: `orders` finds `Orders`.
fn alike(a: &str, b: &str) -> String {
    format!("lower({a}) = lower({b})")
}

fn literal(s: &str) -> String {
    D.quote_literal(s)
}

/// `array_to_string(list, SEP)`.
fn joined(list: &str) -> String {
    format!("array_to_string({list}, chr(31))")
}

/// The items of a [`joined`] cell.
fn items(row: &TextRow, i: usize) -> Vec<String> {
    let text = row.text(i);
    if text.is_empty() {
        return Vec::new();
    }
    text.split(SEP).map(str::to_owned).collect()
}

pub async fn schemas(conn: &dyn Connection) -> Result<Vec<SchemaMeta>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT schema_name, schema_name = current_schema(),
                    schema_name IN ('information_schema', 'pg_catalog')
             FROM duckdb_schemas() WHERE {HERE}
             UNION ALL
             SELECT d.database_name, false, false FROM duckdb_databases() d
             WHERE NOT d.internal AND d.database_name <> current_database()
               AND EXISTS (SELECT 1 FROM duckdb_schemas() s
                           WHERE s.database_name = d.database_name AND s.schema_name = 'main')
               AND d.database_name NOT IN (SELECT schema_name FROM duckdb_schemas() WHERE {HERE})
             ORDER BY 1"
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| SchemaMeta {
            name: r.text(0),
            current: r.flag(1),
            system: r.flag(2),
        })
        .collect())
}

/// `(name, kind code, parent, comment, rows)` of every object in the schema literal `s`.
fn objects_sql(s: &str) -> String {
    let at = listed_as(s, equal);
    format!(
        "SELECT table_name AS name, 'table' AS kind, comment, estimated_size AS rows
         FROM duckdb_tables() WHERE {at}
         UNION ALL
         SELECT view_name, 'view', comment, NULL FROM duckdb_views() WHERE {at} AND NOT internal
         UNION ALL
         SELECT sequence_name, 'sequence', comment, NULL FROM duckdb_sequences() WHERE {at}
         UNION ALL
         SELECT DISTINCT function_name, 'function', comment, NULL FROM duckdb_functions()
         WHERE {at} AND NOT internal AND function_type IN ('macro', 'table_macro')
         UNION ALL
         SELECT type_name, 'type', comment, NULL FROM duckdb_types() WHERE {at} AND NOT internal"
    )
}

fn object_kind(code: &str) -> ObjectKind {
    match code {
        "view" => ObjectKind::View,
        "sequence" => ObjectKind::Sequence,
        "function" => ObjectKind::Function,
        "type" => ObjectKind::Type,
        _ => ObjectKind::Table,
    }
}

pub async fn objects(conn: &dyn Connection, schema: &str) -> Result<Vec<ObjectMeta>> {
    let sql = format!("{} ORDER BY 1, 2", objects_sql(&literal(schema)));
    Ok(query_text(conn, &sql)
        .await?
        .iter()
        .map(|r| ObjectMeta {
            // DuckDB keeps no table sizes, only row estimates.
            rows: r.number(3),
            ..ObjectMeta::new(
                r.text(0),
                object_kind(&r.text(1)),
                r.opt(2).filter(|c| !c.is_empty()),
            )
        })
        .collect())
}

/// The labels of an inline enum type, as DuckDB prints it: `ENUM('a', 'it''s')`.
fn inline_enum(data_type: &str) -> Vec<String> {
    let Some(body) = data_type
        .strip_prefix("ENUM(")
        .and_then(|b| b.strip_suffix(')'))
    else {
        return Vec::new();
    };
    let mut labels = Vec::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\'' {
            continue;
        }
        let mut label = String::new();
        while let Some(c) = chars.next() {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    break;
                }
            }
            label.push(c);
        }
        labels.push(label);
    }
    labels
}

async fn columns(conn: &dyn Connection, at: &str, primary: &[String]) -> Result<Vec<ColumnMeta>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT c.column_name, c.data_type, c.is_nullable, c.column_default, c.comment,
                    {}
             FROM (SELECT * FROM duckdb_columns() WHERE {at}) c
             LEFT JOIN duckdb_types() t
               ON t.database_name = c.database_name AND t.type_name = c.data_type
              AND t.logical_type = 'ENUM' AND NOT t.internal
             ORDER BY c.column_index",
            joined("t.labels")
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let data_type = r.text(1);
            let mut enum_values = items(r, 5);
            if enum_values.is_empty() {
                enum_values = inline_enum(&data_type);
            }
            ColumnMeta {
                primary_key: primary.contains(&r.text(0)),
                name: r.text(0),
                data_type,
                nullable: r.flag(2),
                default: r.opt(3),
                comment: r.opt(4).filter(|c| !c.is_empty()),
                enum_values,
                fields: Vec::new(),
            }
        })
        .collect())
}

/// One row of `duckdb_constraints()`.
struct Constraint {
    kind: String,
    name: String,
    columns: Vec<String>,
    text: String,
    ref_table: String,
    ref_columns: Vec<String>,
}

async fn constraints(conn: &dyn Connection, at: &str) -> Result<Vec<Constraint>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT constraint_type, constraint_name, {}, constraint_text, referenced_table, {}
             FROM duckdb_constraints()
             WHERE {at} AND constraint_type <> 'NOT NULL'
             ORDER BY constraint_index",
            joined("constraint_column_names"),
            joined("referenced_column_names")
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| Constraint {
            kind: r.text(0),
            name: r.text(1),
            columns: items(r, 2),
            text: r.text(3),
            ref_table: r.text(4),
            ref_columns: items(r, 5),
        })
        .collect())
}

/// `[a, b]`, how `duckdb_indexes()` prints the indexed expressions: split at the commas
/// outside parentheses and quotes, quoted names unquoted.
fn index_expressions(text: &str) -> Vec<String> {
    let inner = text
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
        .unwrap_or(text);
    let mut items = Vec::new();
    let (mut depth, mut quote, mut start) = (0u32, None, 0);
    for (i, c) in inner.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth = depth.saturating_sub(1),
            (None, ',') if depth == 0 => {
                items.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(&inner[start..]);
    items
        .into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(
            |s| match s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                Some(name) => name.replace("\"\"", "\""),
                None => s.to_owned(),
            },
        )
        .collect()
}

pub async fn table_details(
    conn: &dyn Connection,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let at = format!(
        "{} AND {}",
        listed_as(&literal(schema), alike),
        alike("table_name", &literal(table))
    );
    let all = constraints(conn, &at).await?;
    let primary: Vec<String> = all
        .iter()
        .find(|c| c.kind == "PRIMARY KEY")
        .map(|c| c.columns.clone())
        .unwrap_or_default();
    let columns = columns(conn, &at, &primary).await?;

    // Keys are enforced by indexes that `duckdb_indexes()` does not list.
    let mut indexes: Vec<IndexMeta> = all
        .iter()
        .filter(|c| c.kind == "PRIMARY KEY" || c.kind == "UNIQUE")
        .map(|c| IndexMeta {
            name: c.name.clone(),
            columns: c.columns.clone(),
            unique: true,
            primary: c.kind == "PRIMARY KEY",
            partial: false,
            size: None,
        })
        .collect();
    let rows = query_text(
        conn,
        &format!(
            "SELECT index_name, is_unique, is_primary, expressions FROM duckdb_indexes()
             WHERE {at} ORDER BY index_name"
        ),
    )
    .await?;
    indexes.extend(rows.iter().map(|r| IndexMeta {
        name: r.text(0),
        unique: r.flag(1),
        primary: r.flag(2),
        partial: false,
        columns: index_expressions(&r.text(3)),
        size: None,
    }));

    let foreign_keys = all
        .iter()
        .filter(|c| c.kind == "FOREIGN KEY")
        .map(|c| ForeignKeyMeta {
            name: c.name.clone(),
            columns: c.columns.clone(),
            // Foreign keys point within their schema.
            ref_schema: schema.to_owned(),
            ref_table: c.ref_table.clone(),
            ref_columns: c.ref_columns.clone(),
        })
        .collect();
    let constraints = all
        .iter()
        .filter_map(|c| {
            let kind = match c.kind.as_str() {
                "PRIMARY KEY" => ConstraintKind::PrimaryKey,
                "UNIQUE" => ConstraintKind::Unique,
                "CHECK" => ConstraintKind::Check,
                _ => return None,
            };
            Some(ConstraintMeta {
                name: c.name.clone(),
                kind,
                columns: c.columns.clone(),
                definition: c.text.clone(),
            })
        })
        .collect();
    let referenced_by = referencing_keys(conn, schema, table).await?;
    Ok(TableDetails {
        columns,
        indexes,
        foreign_keys,
        constraints,
        referenced_by,
    })
}

/// Foreign keys as `(schema, table, name, columns, referenced table, referenced columns)`.
fn foreign_keys_sql(filter: &str) -> String {
    format!(
        "SELECT schema_name, table_name, constraint_name, {}, referenced_table, {}
         FROM duckdb_constraints()
         WHERE {HERE} AND constraint_type = 'FOREIGN KEY' {filter}
         ORDER BY schema_name, table_name, constraint_index
         LIMIT {CATALOG_LIMIT}",
        joined("constraint_column_names"),
        joined("referenced_column_names")
    )
}

pub async fn referencing_keys(
    conn: &dyn Connection,
    schema: &str,
    table: &str,
) -> Result<Vec<ReferencingKey>> {
    let filter = format!(
        "AND lower(schema_name) = lower({}) AND lower(referenced_table) = lower({})",
        literal(schema),
        literal(table)
    );
    Ok(query_text(conn, &foreign_keys_sql(&filter))
        .await?
        .iter()
        .map(|r| ReferencingKey {
            schema: r.text(0),
            table: r.text(1),
            name: r.text(2),
            columns: items(r, 3),
            ref_columns: items(r, 5),
        })
        .collect())
}

pub async fn catalog_columns(conn: &dyn Connection) -> Result<Vec<CatalogColumn>> {
    query_catalog(
        conn,
        &format!(
            "SELECT schema_name, table_name, column_name FROM duckdb_columns()
             WHERE {HERE} AND NOT internal
             ORDER BY schema_name, table_name, column_index
             LIMIT {CATALOG_LIMIT}"
        ),
    )
    .await
}

pub async fn catalog_foreign_keys(conn: &dyn Connection) -> Result<Vec<CatalogForeignKey>> {
    Ok(query_text(conn, &foreign_keys_sql(""))
        .await?
        .iter()
        .map(|r| CatalogForeignKey {
            schema: r.text(0),
            table: r.text(1),
            columns: items(r, 3),
            ref_schema: r.text(0),
            ref_table: r.text(4),
            ref_columns: items(r, 5),
        })
        .collect())
}

pub async fn search_metadata(conn: &dyn Connection, needle: &str) -> Result<Vec<MetadataMatch>> {
    let has = |expr: &str| D.contains(expr, needle);
    let (name, column) = (has("o.name"), has("c.column_name"));
    let objects = format!(
        "SELECT schema_name, table_name AS name, 'table' AS kind, NULL AS source
         FROM duckdb_tables() WHERE {HERE}
         UNION ALL
         SELECT schema_name, view_name, 'view', sql FROM duckdb_views()
         WHERE {HERE} AND NOT internal
         UNION ALL
         SELECT schema_name, sequence_name, 'sequence', NULL FROM duckdb_sequences() WHERE {HERE}
         UNION ALL
         SELECT schema_name, function_name, 'function', macro_definition FROM duckdb_functions()
         WHERE {HERE} AND NOT internal AND function_type IN ('macro', 'table_macro')
         UNION ALL
         SELECT schema_name, type_name, 'type', NULL FROM duckdb_types()
         WHERE {HERE} AND NOT internal"
    );
    let rows = query_text(
        conn,
        &format!(
            "WITH o AS ({objects})
             SELECT DISTINCT o.schema_name, o.name, o.kind, 'name' AS matched, NULL AS col
             FROM o WHERE {name}
             UNION ALL
             SELECT c.schema_name, c.table_name,
                    CASE WHEN EXISTS (SELECT 1 FROM duckdb_views() v WHERE v.{HERE}
                         AND v.schema_name = c.schema_name AND v.view_name = c.table_name)
                         THEN 'view' ELSE 'table' END,
                    'column', c.column_name
             FROM duckdb_columns() c WHERE c.{HERE} AND NOT c.internal AND {column}
             UNION ALL
             SELECT DISTINCT o.schema_name, o.name, o.kind, 'source', NULL
             FROM o WHERE NOT ({name}) AND {}
             ORDER BY 1, 2, 5
             LIMIT {SEARCH_LIMIT}",
            has("o.source")
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| MetadataMatch {
            schema: r.text(0),
            name: r.text(1),
            kind: object_kind(&r.text(2)),
            parent: None,
            matched: MatchKind::from_code(&r.text(3)),
            column: r.opt(4),
        })
        .collect())
}

/// The `CREATE` statement of an object; a table's is followed by its indexes.
pub async fn object_ddl(conn: &dyn Connection, o: &ObjectRef<'_>) -> Result<String> {
    let at = listed_as(&literal(o.schema), equal);
    let name = literal(o.name);
    // Macros and types are printed from their parts, with the name as it must be written.
    let quoted = literal(&D.quote_ident(o.name));
    let sql = match o.kind {
        ObjectKind::Table => format!(
            "SELECT sql FROM (
               SELECT 0 AS n, '' AS i, sql FROM duckdb_tables() WHERE {at} AND table_name = {name}
               UNION ALL
               SELECT 1, index_name, sql FROM duckdb_indexes()
               WHERE {at} AND table_name = {name} AND sql IS NOT NULL)
             ORDER BY n, i"
        ),
        ObjectKind::View => {
            format!("SELECT sql FROM duckdb_views() WHERE {at} AND view_name = {name}")
        }
        ObjectKind::Sequence => {
            format!("SELECT sql FROM duckdb_sequences() WHERE {at} AND sequence_name = {name}")
        }
        ObjectKind::Function => format!(
            "SELECT 'CREATE MACRO ' || {quoted} || '('|| array_to_string(parameters, ', ')
                    || ') AS ' || CASE WHEN function_type = 'table_macro' THEN 'TABLE ' ELSE '' END
                    || macro_definition
             FROM duckdb_functions()
             WHERE {at} AND function_name = {name} AND NOT internal
               AND function_type IN ('macro', 'table_macro')"
        ),
        ObjectKind::Type => format!(
            // `lambda l: …`: the bundled DuckDB rejects the older `l -> …`.
            "SELECT 'CREATE TYPE ' || {quoted} || ' AS ' || CASE WHEN logical_type = 'ENUM'
                    THEN 'ENUM (' || array_to_string(list_transform(labels,
                         lambda l:'''' || replace(l, '''', '''''') || ''''), ', ') || ')'
                    ELSE logical_type END
             FROM duckdb_types() WHERE {at} AND type_name = {name} AND NOT internal"
        ),
        other => return Err(unsupported(&format!("{other:?} objects in DuckDB"))),
    };
    let statements: Vec<String> = query_text(conn, &sql)
        .await?
        .iter()
        .filter_map(|r| r.opt(0))
        .map(|s| format!("{};", s.trim_end().trim_end_matches(';')))
        .collect();
    if statements.is_empty() {
        return Err(Error::Other(format!("{}.{} not found", o.schema, o.name)));
    }
    Ok(statements.join("\n\n"))
}

pub async fn object_properties(conn: &dyn Connection, o: &ObjectRef<'_>) -> Result<Vec<Property>> {
    let at = listed_as(&literal(o.schema), equal);
    let name = literal(o.name);
    let (sql, labels): (String, &[&str]) = match o.kind {
        ObjectKind::Table => (
            format!(
                "SELECT estimated_size, column_count, index_count, temporary, comment
                 FROM duckdb_tables() WHERE {at} AND table_name = {name}"
            ),
            &[
                "Estimated rows",
                "Columns",
                "Indexes",
                "Temporary",
                "Comment",
            ],
        ),
        ObjectKind::View => (
            format!(
                "SELECT column_count, temporary, comment FROM duckdb_views()
                 WHERE {at} AND view_name = {name}"
            ),
            &["Columns", "Temporary", "Comment"],
        ),
        ObjectKind::Sequence => (
            format!(
                "SELECT start_value, increment_by, min_value, max_value, cycle, last_value
                 FROM duckdb_sequences() WHERE {at} AND sequence_name = {name}"
            ),
            &[
                "Start",
                "Increment",
                "Minimum",
                "Maximum",
                "Cycle",
                "Last value",
            ],
        ),
        _ => return Ok(Vec::new()),
    };
    let rows = query_text(conn, &sql).await?;
    let Some(row) = rows.first() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        if let Some(value) = row.opt(i).filter(|v| !v.is_empty()) {
            out.push(Property::new(*label, value));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_catalog_text() {
        assert_eq!(
            inline_enum("ENUM('a', 'it''s', 'x,y')"),
            ["a", "it's", "x,y"]
        );
        assert_eq!(inline_enum("VARCHAR"), Vec::<String>::new());
        assert_eq!(index_expressions("[customer_id]"), ["customer_id"]);
        assert_eq!(index_expressions("[a, \"B c\"]"), ["a", "B c"]);
        assert_eq!(index_expressions("[]"), Vec::<String>::new());
        assert_eq!(
            index_expressions("[coalesce(a, b), \"x, y\"]"),
            ["coalesce(a, b)", "x, y"]
        );
    }
}
