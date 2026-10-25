//! Navigator metadata from the `system` tables. Databases are the schemas; ClickHouse has
//! no foreign keys or unique constraints, and its primary key orders the data without
//! making rows unique.

use platypus_core::{
    CATALOG_LIMIT, CatalogColumn, ColumnMeta, ConstraintKind, ConstraintMeta, IndexMeta, MatchKind,
    MetadataMatch, ObjectKind, ObjectMeta, ObjectRef, Property, Result, SEARCH_LIMIT, SchemaMeta,
    TableDetails, TextRowExt, query_catalog, query_text, unsupported,
};
use platypus_sql::Dialect;

use crate::ClickHouseConnection;

const D: Dialect = Dialect::ClickHouse;
const SYSTEM: &str = "('system', 'INFORMATION_SCHEMA', 'information_schema')";

fn lit(s: &str) -> String {
    D.quote_literal(s)
}

pub async fn schemas(conn: &ClickHouseConnection) -> Result<Vec<SchemaMeta>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT name, name IN {SYSTEM}, name = currentDatabase()
             FROM system.databases ORDER BY name"
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| SchemaMeta {
            name: r.text(0),
            system: r.flag(1),
            current: r.flag(2),
        })
        .collect())
}

/// The object kind of a table engine.
fn kind_of(engine: &str) -> ObjectKind {
    match engine {
        "View" | "LiveView" | "WindowView" => ObjectKind::View,
        "MaterializedView" => ObjectKind::MaterializedView,
        "Dictionary" => ObjectKind::Dictionary,
        _ => ObjectKind::Table,
    }
}

pub async fn objects(conn: &ClickHouseConnection, schema: &str) -> Result<Vec<ObjectMeta>> {
    // `.inner` tables hold the data of materialized views and belong to them.
    let rows = query_text(
        conn,
        &format!(
            "SELECT name, engine, nullIf(comment, ''), total_rows, total_bytes
             FROM system.tables
             WHERE database = {} AND NOT is_temporary AND NOT startsWith(name, '.inner')
             ORDER BY name",
            lit(schema)
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let mut object = ObjectMeta::new(r.text(0), kind_of(&r.text(1)), r.opt(2));
            object.rows = r.number(3);
            object.size = r.number(4);
            object
        })
        .collect())
}

/// The element type's values of an `Enum8('a' = 1, 'b' = 2)`, possibly wrapped in
/// `Nullable` or `LowCardinality`; empty for other types.
fn enum_values(data_type: &str) -> Vec<String> {
    let Some(start) = data_type
        .find("Enum8(")
        .or_else(|| data_type.find("Enum16("))
    else {
        return Vec::new();
    };
    let mut values = Vec::new();
    let mut chars = data_type[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                let mut value = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => value.extend(chars.next()),
                        '\'' => break,
                        c => value.push(c),
                    }
                }
                values.push(value);
            }
            ')' => break,
            _ => {}
        }
    }
    values
}

/// The expressions of a key such as `id, toDate(at)`, split at top-level commas.
fn key_parts(key: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let (mut depth, mut start, mut quote) = (0_i32, 0, None);
    for (i, c) in key.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '`' | '"') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(key[start..i].trim().to_owned());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = key[start..].trim();
    if !last.is_empty() {
        parts.push(last.to_owned());
    }
    parts
}

/// The `CONSTRAINT name CHECK expr` lines of a `SHOW CREATE TABLE` statement.
fn check_constraints(ddl: &str) -> Vec<ConstraintMeta> {
    ddl.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("CONSTRAINT ")?;
            let (name, definition) = rest.split_once(' ')?;
            let definition = definition.trim_end_matches(',');
            definition.starts_with("CHECK ").then(|| ConstraintMeta {
                name: name.trim_matches('`').to_owned(),
                kind: ConstraintKind::Check,
                columns: Vec::new(),
                definition: definition.to_owned(),
            })
        })
        .collect()
}

fn column_meta(r: &platypus_core::TextRow) -> ColumnMeta {
    let data_type = r.text(1);
    let default = match (r.text(2).as_str(), r.opt(3)) {
        (_, None) | ("", _) => None,
        ("DEFAULT", Some(expr)) => Some(expr),
        (kind, Some(expr)) => Some(format!("{kind} {expr}")),
    };
    ColumnMeta {
        name: r.text(0),
        nullable: data_type.starts_with("Nullable(")
            || data_type.starts_with("LowCardinality(Nullable("),
        enum_values: enum_values(&data_type),
        data_type,
        default,
        primary_key: r.flag(5),
        comment: r.opt(4),
        fields: Vec::new(),
    }
}

pub async fn table_details(
    conn: &ClickHouseConnection,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let (s, t) = (lit(schema), lit(table));
    let columns = query_text(
        conn,
        &format!(
            "SELECT name, type, default_kind, nullIf(default_expression, ''),
                    nullIf(comment, ''), is_in_primary_key
             FROM system.columns WHERE database = {s} AND table = {t} ORDER BY position"
        ),
    )
    .await?;
    let keys = query_text(
        conn,
        &format!(
            "SELECT primary_key, sorting_key, engine FROM system.tables
             WHERE database = {s} AND name = {t}"
        ),
    )
    .await?;
    let skipping = query_text(
        conn,
        &format!(
            "SELECT name, expr, data_compressed_bytes FROM system.data_skipping_indices
             WHERE database = {s} AND table = {t} ORDER BY name"
        ),
    )
    .await?;

    let mut details = TableDetails {
        columns: columns.iter().map(column_meta).collect(),
        ..TableDetails::default()
    };
    let Some(keys) = keys.first() else {
        return Ok(details);
    };
    let (primary, sorting) = (key_parts(&keys.text(0)), key_parts(&keys.text(1)));
    if !primary.is_empty() {
        details.indexes.push(IndexMeta {
            name: "PRIMARY KEY".to_owned(),
            columns: primary.clone(),
            unique: false,
            primary: true,
            size: None,
        });
        details.constraints.push(ConstraintMeta {
            name: "PRIMARY KEY".to_owned(),
            kind: ConstraintKind::PrimaryKey,
            definition: format!("PRIMARY KEY ({})", primary.join(", ")),
            columns: primary.clone(),
        });
    }
    if !sorting.is_empty() && sorting != primary {
        details.indexes.push(IndexMeta {
            name: "ORDER BY".to_owned(),
            columns: sorting,
            unique: false,
            primary: false,
            size: None,
        });
    }
    details.indexes.extend(skipping.iter().map(|r| IndexMeta {
        name: r.text(0),
        columns: vec![r.text(1)],
        unique: false,
        primary: false,
        size: r.number(2),
    }));
    if kind_of(&keys.text(2)) == ObjectKind::Table {
        let ddl = show_create(conn, schema, table, ObjectKind::Table).await?;
        details.constraints.extend(check_constraints(&ddl));
    }
    Ok(details)
}

pub async fn catalog_columns(conn: &ClickHouseConnection) -> Result<Vec<CatalogColumn>> {
    query_catalog(
        conn,
        &format!(
            "SELECT database, table, name FROM system.columns
             WHERE database NOT IN {SYSTEM} AND database != ''
             ORDER BY database, table, position
             LIMIT {CATALOG_LIMIT}"
        ),
    )
    .await
}

async fn show_create(
    conn: &ClickHouseConnection,
    schema: &str,
    name: &str,
    kind: ObjectKind,
) -> Result<String> {
    let what = if kind == ObjectKind::Dictionary {
        "DICTIONARY"
    } else {
        "TABLE"
    };
    let rows = query_text(
        conn,
        &format!("SHOW CREATE {what} {}", D.qualified(Some(schema), name)),
    )
    .await?;
    Ok(rows.first().map(|r| r.text(0)).unwrap_or_default())
}

pub async fn object_ddl(conn: &ClickHouseConnection, o: &ObjectRef<'_>) -> Result<String> {
    if !o.kind.is_relation() {
        return Err(unsupported("DDL of this object kind in ClickHouse"));
    }
    let ddl = show_create(conn, o.schema, o.name, o.kind).await?;
    Ok(format!("{};", ddl.trim_end()))
}

pub async fn object_properties(
    conn: &ClickHouseConnection,
    o: &ObjectRef<'_>,
) -> Result<Vec<Property>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT engine, toString(total_rows), formatReadableSize(total_bytes),
                    nullIf(partition_key, ''), nullIf(sorting_key, ''), nullIf(primary_key, ''),
                    toString(metadata_modification_time), nullIf(comment, '')
             FROM system.tables WHERE database = {} AND name = {}",
            lit(o.schema),
            lit(o.name)
        ),
    )
    .await?;
    let Some(r) = rows.into_iter().next() else {
        return Ok(Vec::new());
    };
    let labels = [
        "Engine",
        "Rows",
        "Size",
        "Partition key",
        "Sorting key",
        "Primary key",
        "Modified",
        "Comment",
    ];
    Ok(labels
        .iter()
        .enumerate()
        .filter_map(|(i, label)| r.opt(i).map(|v| Property::new(*label, v)))
        .collect())
}

pub async fn search_metadata(
    conn: &ClickHouseConnection,
    needle: &str,
) -> Result<Vec<MetadataMatch>> {
    let has = |expr: &str| D.contains(expr, needle);
    let (name, column, source) = (has("t.name"), has("c.name"), has("t.as_select"));
    let rows = query_text(
        conn,
        &format!(
            "SELECT t.database, t.name, t.engine, 'name', CAST(NULL AS Nullable(String))
             FROM system.tables t WHERE t.database NOT IN {SYSTEM} AND {name}
             UNION ALL
             SELECT c.database, c.table, t.engine, 'column', c.name
             FROM system.columns c
             JOIN system.tables t ON t.database = c.database AND t.name = c.table
             WHERE c.database NOT IN {SYSTEM} AND {column}
             UNION ALL
             SELECT t.database, t.name, t.engine, 'source', CAST(NULL AS Nullable(String))
             FROM system.tables t
             WHERE t.database NOT IN {SYSTEM} AND t.engine IN ('View', 'MaterializedView')
               AND NOT {name} AND {source}
             ORDER BY 1, 2, 5
             LIMIT {SEARCH_LIMIT}"
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| MetadataMatch {
            schema: r.text(0),
            name: r.text(1),
            kind: kind_of(&r.text(2)),
            parent: None,
            matched: MatchKind::from_code(&r.text(3)),
            column: r.opt(4),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_enum_values() {
        assert_eq!(
            enum_values("Nullable(Enum8('a' = 1, 'it\\'s' = 2))"),
            ["a", "it's"]
        );
        assert_eq!(enum_values("String"), Vec::<String>::new());
    }

    #[test]
    fn splits_keys_at_top_level_commas() {
        assert_eq!(
            key_parts("id, toDate(at, 'UTC'), x"),
            ["id", "toDate(at, 'UTC')", "x"]
        );
        assert_eq!(key_parts("id"), ["id"]);
        assert_eq!(key_parts(""), Vec::<String>::new());
    }

    #[test]
    fn finds_check_constraints() {
        let ddl = "CREATE TABLE p.t\n(\n    `qty` Int32,\n    CONSTRAINT qty_ok CHECK qty >= 0,\n    \
                   CONSTRAINT a ASSUME qty < 9\n)\nENGINE = MergeTree";
        let checks = check_constraints(ddl);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].name, "qty_ok");
        assert_eq!(checks[0].definition, "CHECK qty >= 0");
    }

    #[test]
    fn maps_engines_to_kinds() {
        assert_eq!(kind_of("MergeTree"), ObjectKind::Table);
        assert_eq!(kind_of("View"), ObjectKind::View);
        assert_eq!(kind_of("MaterializedView"), ObjectKind::MaterializedView);
        assert_eq!(kind_of("Dictionary"), ObjectKind::Dictionary);
    }
}
