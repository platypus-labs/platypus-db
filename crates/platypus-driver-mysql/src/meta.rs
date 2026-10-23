//! Navigator metadata from `information_schema`, valid for MySQL and MariaDB.

use platypus_core::{
    CATALOG_LIMIT, CatalogColumn, ColumnMeta, ForeignKeyMeta, IndexMeta, ObjectKind, ObjectMeta,
    Result, SchemaMeta, TableDetails, TextRowExt, group_sorted, query_catalog, query_text,
};

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
            "SELECT TABLE_NAME AS name, TABLE_TYPE AS kind, TABLE_COMMENT AS comment
             FROM information_schema.TABLES WHERE TABLE_SCHEMA = {schema}
             UNION ALL
             SELECT ROUTINE_NAME, ROUTINE_TYPE, ROUTINE_COMMENT
             FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = {schema}
             ORDER BY name"
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| ObjectMeta {
            name: r.text(0),
            kind: match r.text(1).as_str() {
                "VIEW" | "SYSTEM VIEW" => ObjectKind::View,
                "SEQUENCE" => ObjectKind::Sequence,
                "FUNCTION" => ObjectKind::Function,
                "PROCEDURE" => ObjectKind::Procedure,
                _ => ObjectKind::Table,
            },
            // Views carry the comment `VIEW`; InnoDB may append its own notes.
            comment: r.opt(2).filter(|c| !c.is_empty() && c != "VIEW"),
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

    Ok(TableDetails {
        columns,
        indexes,
        foreign_keys,
    })
}
