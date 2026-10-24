//! Navigator metadata from `sqlite_master` and the `pragma_*` table-valued functions.

use platypus_core::{
    CATALOG_LIMIT, CatalogColumn, ColumnMeta, Connection, ForeignKeyMeta, IndexMeta, ObjectKind,
    ObjectMeta, ReferencingKey, Result, SchemaMeta, TableDetails, TextRowExt, group_sorted,
    query_catalog, query_text,
};

fn literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

pub async fn schemas(conn: &dyn Connection) -> Result<Vec<SchemaMeta>> {
    let rows = query_text(conn, "SELECT name FROM pragma_database_list ORDER BY seq").await?;
    Ok(rows
        .iter()
        .map(|r| SchemaMeta {
            current: r.text(0) == "main",
            name: r.text(0),
            system: false,
        })
        .collect())
}

pub async fn catalog_columns(conn: &dyn Connection) -> Result<Vec<CatalogColumn>> {
    let mut columns = Vec::new();
    for schema in schemas(conn).await? {
        let (lit, id) = (literal(&schema.name), ident(&schema.name));
        columns.extend(
            query_catalog(
                conn,
                &format!(
                    "SELECT {lit}, m.name, p.name
                     FROM {id}.sqlite_master m JOIN pragma_table_info(m.name, {lit}) p
                     WHERE m.type IN ('table', 'view') AND m.name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
                     ORDER BY m.name, p.cid
                     LIMIT {CATALOG_LIMIT}"
                ),
            )
            .await?,
        );
    }
    columns.truncate(CATALOG_LIMIT);
    Ok(columns)
}

pub async fn objects(conn: &dyn Connection, schema: &str) -> Result<Vec<ObjectMeta>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT name, type FROM {}.sqlite_master
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
             ORDER BY name",
            ident(schema)
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| ObjectMeta {
            name: r.text(0),
            kind: if r.text(1) == "view" {
                ObjectKind::View
            } else {
                ObjectKind::Table
            },
            comment: None,
        })
        .collect())
}

pub async fn table_details(
    conn: &dyn Connection,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let (schema_lit, table_lit) = (literal(schema), literal(table));

    // `hidden` is 1 for hidden virtual-table columns, 2 and 3 for generated columns.
    let columns: Vec<ColumnMeta> = query_text(
        conn,
        &format!(
            "SELECT name, type, \"notnull\", dflt_value, pk
             FROM pragma_table_xinfo({table_lit}, {schema_lit})
             WHERE hidden <> 1 ORDER BY cid"
        ),
    )
    .await?
    .iter()
    .map(|r| ColumnMeta {
        name: r.text(0),
        data_type: r.text(1),
        nullable: !r.flag(2),
        default: r.opt(3),
        primary_key: r.text(4) != "0",
        comment: None,
        enum_values: Vec::new(),
    })
    .collect();

    let rows = query_text(
        conn,
        &format!(
            "SELECT il.name, il.\"unique\", il.origin = 'pk', coalesce(ii.name, '(expression)')
             FROM pragma_index_list({table_lit}, {schema_lit}) il
             JOIN pragma_index_info(il.name, {schema_lit}) ii
             ORDER BY il.origin = 'pk' DESC, il.name, ii.seqno"
        ),
    )
    .await?;
    let mut indexes = group_sorted(
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
    // An `INTEGER PRIMARY KEY` is the rowid itself and has no index of its own.
    if !indexes.iter().any(|i| i.primary) && columns.iter().any(|c| c.primary_key) {
        indexes.insert(
            0,
            IndexMeta {
                name: "PRIMARY KEY".to_owned(),
                columns: columns
                    .iter()
                    .filter(|c| c.primary_key)
                    .map(|c| c.name.clone())
                    .collect(),
                unique: true,
                primary: true,
            },
        );
    }

    let rows = query_text(
        conn,
        &format!(
            "SELECT id, \"from\", \"table\", coalesce(\"to\", '')
             FROM pragma_foreign_key_list({table_lit}, {schema_lit})
             ORDER BY id, seq"
        ),
    )
    .await?;
    let foreign_keys = group_sorted(
        rows.iter().map(|r| {
            (
                r.text(0),
                ForeignKeyMeta {
                    name: format!("fk_{}_{}", table, r.text(0)),
                    columns: vec![r.text(1)],
                    ref_schema: schema.to_owned(),
                    ref_table: r.text(2),
                    // Empty when the key references the parent's primary key implicitly.
                    ref_columns: vec![r.text(3)],
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

/// SQLite keys only point within their own database, so the other tables of `schema` are
/// searched.
pub async fn referencing_keys(
    conn: &dyn Connection,
    schema: &str,
    table: &str,
) -> Result<Vec<ReferencingKey>> {
    let schema_lit = literal(schema);
    let rows = query_text(
        conn,
        &format!(
            "SELECT m.name || char(31) || f.id, m.name, f.id, f.\"from\", coalesce(f.\"to\", '')
             FROM {}.sqlite_master m
             JOIN pragma_foreign_key_list(m.name, {schema_lit}) f
             WHERE m.type = 'table' AND f.\"table\" = {} COLLATE NOCASE
             ORDER BY m.name, f.id, f.seq",
            ident(schema),
            literal(table)
        ),
    )
    .await?;
    let mut keys = group_sorted(
        rows.iter().map(|r| {
            (
                r.text(0),
                ReferencingKey {
                    schema: schema.to_owned(),
                    table: r.text(1),
                    name: format!("fk_{}_{}", r.text(1), r.text(2)),
                    columns: vec![r.text(3)],
                    ref_columns: vec![r.text(4)],
                },
            )
        }),
        |key, row| {
            key.columns.extend(row.columns);
            key.ref_columns.extend(row.ref_columns);
        },
    );
    // A key without target columns points at the primary key.
    if keys
        .iter()
        .any(|k| k.ref_columns.iter().any(String::is_empty))
    {
        let primary: Vec<String> = query_text(
            conn,
            &format!(
                "SELECT name FROM pragma_table_info({}, {schema_lit}) WHERE pk > 0 ORDER BY pk",
                literal(table)
            ),
        )
        .await?
        .iter()
        .map(|r| r.text(0))
        .collect();
        for key in &mut keys {
            if key.ref_columns.iter().any(String::is_empty) {
                key.ref_columns.clone_from(&primary);
            }
        }
    }
    Ok(keys)
}
