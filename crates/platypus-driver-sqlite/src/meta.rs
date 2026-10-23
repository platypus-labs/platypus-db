//! Navigator metadata from `sqlite_master` and the `pragma_*` table-valued functions.

use platypus_core::{
    ColumnMeta, Connection, ForeignKeyMeta, IndexMeta, ObjectKind, ObjectMeta, Result, SchemaMeta,
    TableDetails, TextRowExt, group_sorted, query_text,
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
