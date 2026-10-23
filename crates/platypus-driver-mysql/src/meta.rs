//! Navigator metadata from `information_schema`, valid for MySQL and MariaDB.

use platypus_core::{
    CATALOG_LIMIT, CatalogColumn, ColumnMeta, ConstraintKind, ConstraintMeta, Error,
    ForeignKeyMeta, IndexMeta, ObjectKind, ObjectMeta, ObjectRef, Property, ReferenceMeta, Result,
    SchemaMeta, TableDetails, TextRow, TextRowExt, group_sorted, query_catalog, query_text,
    unsupported,
};
use platypus_sql::Dialect;

use crate::MySqlConnection;

pub async fn schemas(conn: &MySqlConnection) -> Result<Vec<SchemaMeta>> {
    let rows = query_text(
        conn,
        "SELECT SCHEMA_NAME,
                SCHEMA_NAME IN ('mysql', 'information_schema', 'performance_schema', 'sys'),
                SCHEMA_NAME = DATABASE()
         FROM information_schema.SCHEMATA
         ORDER BY SCHEMA_NAME",
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

pub async fn catalog_columns(conn: &MySqlConnection) -> Result<Vec<CatalogColumn>> {
    query_catalog(
        conn,
        &format!(
            "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME
             FROM information_schema.COLUMNS
             WHERE TABLE_SCHEMA NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys')
             ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION
             LIMIT {CATALOG_LIMIT}"
        ),
    )
    .await
}

pub async fn objects(conn: &MySqlConnection, schema: &str) -> Result<Vec<ObjectMeta>> {
    let schema = conn.literal(schema);
    let rows = query_text(
        conn,
        &format!(
            "SELECT TABLE_NAME AS name, TABLE_TYPE AS kind, TABLE_COMMENT AS comment,
                    NULL AS parent
             FROM information_schema.TABLES WHERE TABLE_SCHEMA = {schema}
             UNION ALL
             SELECT ROUTINE_NAME, ROUTINE_TYPE, ROUTINE_COMMENT, NULL
             FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = {schema}
             UNION ALL
             SELECT TRIGGER_NAME, 'TRIGGER', NULL, EVENT_OBJECT_TABLE
             FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA = {schema}
             UNION ALL
             SELECT EVENT_NAME, 'EVENT', EVENT_COMMENT, NULL
             FROM information_schema.EVENTS WHERE EVENT_SCHEMA = {schema}
             ORDER BY name"
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let kind = match r.text(1).as_str() {
                "VIEW" | "SYSTEM VIEW" => ObjectKind::View,
                "SEQUENCE" => ObjectKind::Sequence,
                "FUNCTION" => ObjectKind::Function,
                "PROCEDURE" => ObjectKind::Procedure,
                "TRIGGER" => ObjectKind::Trigger,
                "EVENT" => ObjectKind::Event,
                _ => ObjectKind::Table,
            };
            // Views carry the comment `VIEW`; InnoDB may append its own notes.
            let comment = r.opt(2).filter(|c| !c.is_empty() && c != "VIEW");
            ObjectMeta {
                parent: r.opt(3),
                ..ObjectMeta::new(r.text(0), kind, comment)
            }
        })
        .collect())
}

pub async fn table_details(
    conn: &MySqlConnection,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let filter = format!(
        "TABLE_SCHEMA = {} AND TABLE_NAME = {}",
        conn.literal(schema),
        conn.literal(table)
    );

    let columns = query_text(
        conn,
        &format!(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE = 'YES', COLUMN_DEFAULT,
                    COLUMN_KEY = 'PRI', COLUMN_COMMENT
             FROM information_schema.COLUMNS WHERE {filter}
             ORDER BY ORDINAL_POSITION"
        ),
    )
    .await?
    .iter()
    .map(|r| ColumnMeta {
        name: r.text(0),
        data_type: r.text(1),
        nullable: r.flag(2),
        default: r.opt(3),
        primary_key: r.flag(4),
        comment: r.opt(5).filter(|c| !c.is_empty()),
    })
    .collect();

    // One row per indexed column; functional key parts have no column name.
    let rows = query_text(
        conn,
        &format!(
            "SELECT INDEX_NAME, NON_UNIQUE = 0, INDEX_NAME = 'PRIMARY',
                    COALESCE(COLUMN_NAME, '(expression)')
             FROM information_schema.STATISTICS WHERE {filter}
             ORDER BY INDEX_NAME = 'PRIMARY' DESC, INDEX_NAME, SEQ_IN_INDEX"
        ),
    )
    .await?;
    let indexes = group_sorted(
        rows.iter().map(|r| {
            (
                r.text(0),
                IndexMeta {
                    name: r.text(0),
                    columns: vec![r.text(3)],
                    unique: r.flag(1),
                    primary: r.flag(2),
                },
            )
        }),
        |index, row| index.columns.extend(row.columns),
    );

    let rows = query_text(
        conn,
        &format!(
            "SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA,
                    REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME
             FROM information_schema.KEY_COLUMN_USAGE
             WHERE {filter} AND REFERENCED_TABLE_NAME IS NOT NULL
             ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION"
        ),
    )
    .await?;
    let foreign_keys = group_sorted(
        rows.iter().map(|r| {
            (
                r.text(0),
                ForeignKeyMeta {
                    name: r.text(0),
                    columns: vec![r.text(1)],
                    ref_schema: r.text(2),
                    ref_table: r.text(3),
                    ref_columns: vec![r.text(4)],
                },
            )
        }),
        |fk, row| {
            fk.columns.extend(row.columns);
            fk.ref_columns.extend(row.ref_columns);
        },
    );

    let constraints = constraints(conn, schema, table).await?;
    let referenced_by = referenced_by(conn, schema, table).await?;

    Ok(TableDetails {
        columns,
        indexes,
        foreign_keys,
        constraints,
        referenced_by,
    })
}

/// Primary key, unique, check and exclusion constraints.
async fn constraints(
    conn: &MySqlConnection,
    schema: &str,
    table: &str,
) -> Result<Vec<ConstraintMeta>> {
    // One row per constraint column; checks have no columns. MySQL 8.0.16+ and MariaDB
    // 10.2+ have CHECK_CONSTRAINTS; check names are unique per schema there.
    let rows = query_text(
        conn,
        &format!(
            "SELECT tc.CONSTRAINT_NAME, tc.CONSTRAINT_TYPE, k.COLUMN_NAME, cc.CHECK_CLAUSE
             FROM information_schema.TABLE_CONSTRAINTS tc
             LEFT JOIN information_schema.KEY_COLUMN_USAGE k
               ON k.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA AND k.TABLE_NAME = tc.TABLE_NAME
              AND k.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
             LEFT JOIN information_schema.CHECK_CONSTRAINTS cc
               ON cc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
              AND cc.CONSTRAINT_NAME = tc.CONSTRAINT_NAME AND tc.CONSTRAINT_TYPE = 'CHECK'
             WHERE tc.TABLE_SCHEMA = {} AND tc.TABLE_NAME = {}
               AND tc.CONSTRAINT_TYPE IN ('PRIMARY KEY', 'UNIQUE', 'CHECK')
             ORDER BY tc.CONSTRAINT_TYPE = 'PRIMARY KEY' DESC, tc.CONSTRAINT_NAME,
                      k.ORDINAL_POSITION",
            conn.literal(schema),
            conn.literal(table)
        ),
    )
    .await?;
    let mut constraints = group_sorted(
        rows.iter().map(|r| {
            let kind = match r.text(1).as_str() {
                "PRIMARY KEY" => ConstraintKind::PrimaryKey,
                "UNIQUE" => ConstraintKind::Unique,
                _ => ConstraintKind::Check,
            };
            (
                r.text(0),
                ConstraintMeta {
                    name: r.text(0),
                    kind,
                    columns: r.opt(2).into_iter().collect(),
                    definition: r.opt(3).map(|c| format!("CHECK ({c})")).unwrap_or_default(),
                },
            )
        }),
        |c, row| c.columns.extend(row.columns),
    );
    for c in &mut constraints {
        if c.definition.is_empty() {
            let columns: Vec<_> = c
                .columns
                .iter()
                .map(|n| Dialect::MySql.quote_ident(n))
                .collect();
            let keyword = if c.kind == ConstraintKind::PrimaryKey {
                "PRIMARY KEY"
            } else {
                "UNIQUE"
            };
            c.definition = format!("{keyword} ({})", columns.join(", "));
        }
    }
    Ok(constraints)
}

/// Foreign keys of other tables that point at this one.
async fn referenced_by(
    conn: &MySqlConnection,
    schema: &str,
    table: &str,
) -> Result<Vec<ReferenceMeta>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT CONSTRAINT_NAME, TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, REFERENCED_COLUMN_NAME
             FROM information_schema.KEY_COLUMN_USAGE
             WHERE REFERENCED_TABLE_SCHEMA = {} AND REFERENCED_TABLE_NAME = {}
             ORDER BY TABLE_SCHEMA, TABLE_NAME, CONSTRAINT_NAME, ORDINAL_POSITION",
            conn.literal(schema),
            conn.literal(table)
        ),
    )
    .await?;
    let referenced_by = group_sorted(
        rows.iter().map(|r| {
            (
                format!("{}\u{1f}{}\u{1f}{}", r.text(1), r.text(2), r.text(0)),
                ReferenceMeta {
                    name: r.text(0),
                    schema: r.text(1),
                    table: r.text(2),
                    columns: vec![r.text(3)],
                    ref_columns: vec![r.text(4)],
                },
            )
        }),
        |reference, row| {
            reference.columns.extend(row.columns);
            reference.ref_columns.extend(row.ref_columns);
        },
    );
    Ok(referenced_by)
}

/// `SHOW CREATE ...` for the object, and the column of its output that holds the statement.
fn show_create(kind: ObjectKind) -> Option<(&'static str, usize)> {
    Some(match kind {
        ObjectKind::Table => ("TABLE", 1),
        ObjectKind::View => ("VIEW", 1),
        ObjectKind::Sequence => ("SEQUENCE", 1),
        ObjectKind::Function => ("FUNCTION", 2),
        ObjectKind::Procedure => ("PROCEDURE", 2),
        ObjectKind::Trigger => ("TRIGGER", 2),
        ObjectKind::Event => ("EVENT", 3),
        _ => return None,
    })
}

pub async fn object_ddl(conn: &MySqlConnection, o: &ObjectRef<'_>) -> Result<String> {
    let (what, column) = show_create(o.kind)
        .ok_or_else(|| unsupported(&format!("{:?} objects in MySQL", o.kind)))?;
    let rows = query_text(
        conn,
        &format!(
            "SHOW CREATE {what} {}",
            Dialect::MySql.qualified(Some(o.schema), o.name)
        ),
    )
    .await?;
    let ddl = rows
        .first()
        .and_then(|r| r.opt(column))
        .ok_or_else(|| Error::Other(format!("no definition for {}.{}", o.schema, o.name)))?;
    let ddl = ddl.trim_end();
    Ok(if ddl.ends_with(';') {
        ddl.to_owned()
    } else {
        format!("{ddl};")
    })
}

/// `1536` → `1.5 KB`.
fn size(bytes: &str) -> String {
    let Ok(mut n) = bytes.parse::<f64>() else {
        return bytes.to_owned();
    };
    for unit in ["bytes", "KB", "MB", "GB"] {
        if n < 1024.0 {
            return if unit == "bytes" {
                format!("{n} {unit}")
            } else {
                format!("{n:.1} {unit}")
            };
        }
        n /= 1024.0;
    }
    format!("{n:.1} TB")
}

pub async fn object_properties(conn: &MySqlConnection, o: &ObjectRef<'_>) -> Result<Vec<Property>> {
    let (schema, name) = (conn.literal(o.schema), conn.literal(o.name));
    let props = |rows: Vec<TextRow>, labels: &[&str]| -> Vec<Property> {
        let Some(r) = rows.into_iter().next() else {
            return Vec::new();
        };
        labels
            .iter()
            .enumerate()
            .filter_map(|(i, label)| r.opt(i).map(|v| Property::new(*label, v)))
            .collect()
    };
    Ok(match o.kind {
        ObjectKind::Table => {
            let rows = query_text(
                conn,
                &format!(
                    "SELECT ENGINE, TABLE_ROWS, DATA_LENGTH, INDEX_LENGTH, TABLE_COLLATION,
                            AUTO_INCREMENT, CREATE_TIME, UPDATE_TIME
                     FROM information_schema.TABLES
                     WHERE TABLE_SCHEMA = {schema} AND TABLE_NAME = {name}"
                ),
            )
            .await?;
            let mut out = props(
                rows,
                &[
                    "Engine",
                    "Estimated rows",
                    "Data size",
                    "Indexes size",
                    "Collation",
                    "Next AUTO_INCREMENT",
                    "Created",
                    "Updated",
                ],
            );
            for p in &mut out {
                if p.name.ends_with("size") {
                    p.value = size(&p.value);
                }
            }
            out
        }
        ObjectKind::View => props(
            query_text(
                conn,
                &format!(
                    "SELECT DEFINER, SECURITY_TYPE, IS_UPDATABLE FROM information_schema.VIEWS
                     WHERE TABLE_SCHEMA = {schema} AND TABLE_NAME = {name}"
                ),
            )
            .await?,
            &["Definer", "Security", "Updatable"],
        ),
        ObjectKind::Function | ObjectKind::Procedure => props(
            query_text(
                conn,
                &format!(
                    "SELECT DEFINER, SECURITY_TYPE, IS_DETERMINISTIC,
                            IF(ROUTINE_TYPE = 'FUNCTION', DTD_IDENTIFIER, NULL),
                            CREATED, LAST_ALTERED
                     FROM information_schema.ROUTINES
                     WHERE ROUTINE_SCHEMA = {schema} AND ROUTINE_NAME = {name}"
                ),
            )
            .await?,
            &[
                "Definer",
                "Security",
                "Deterministic",
                "Returns",
                "Created",
                "Last altered",
            ],
        ),
        _ => Vec::new(),
    })
}
