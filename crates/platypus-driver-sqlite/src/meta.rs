//! Navigator metadata from `sqlite_master` and the `pragma_*` table-valued functions.

use std::fmt::Write as _;

use platypus_core::{
    CATALOG_LIMIT, CatalogColumn, CatalogForeignKey, ColumnMeta, Connection, ConstraintKind,
    ConstraintMeta, Error, ForeignKeyMeta, IndexMeta, ObjectKind, ObjectMeta, ObjectRef,
    ReferenceMeta, Result, SchemaMeta, TableDetails, TextRowExt, group_sorted, query_catalog,
    query_text, unsupported,
};
use platypus_sql::Dialect;

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

pub async fn catalog_foreign_keys(conn: &dyn Connection) -> Result<Vec<CatalogForeignKey>> {
    let mut keys = Vec::new();
    for schema in schemas(conn).await? {
        let (lit, id) = (literal(&schema.name), ident(&schema.name));
        // A key without target columns references the parent's primary key.
        let rows = query_text(
            conn,
            &format!(
                "SELECT m.name, f.id, f.\"from\", f.\"table\",
                        coalesce(f.\"to\", (SELECT p.name FROM pragma_table_info(f.\"table\", {lit}) p
                                            WHERE p.pk = f.seq + 1), '')
                 FROM {id}.sqlite_master m JOIN pragma_foreign_key_list(m.name, {lit}) f
                 WHERE m.type = 'table'
                 ORDER BY m.name, f.id, f.seq
                 LIMIT {CATALOG_LIMIT}"
            ),
        )
        .await?;
        keys.extend(group_sorted(
            rows.iter().map(|r| {
                (
                    format!("{}\u{0}{}", r.text(0), r.text(1)),
                    CatalogForeignKey {
                        schema: schema.name.clone(),
                        table: r.text(0),
                        columns: vec![r.text(2)],
                        ref_schema: schema.name.clone(),
                        ref_table: r.text(3),
                        ref_columns: vec![r.text(4)],
                    },
                )
            }),
            |fk, row| {
                fk.columns.extend(row.columns);
                fk.ref_columns.extend(row.ref_columns);
            },
        ));
    }
    keys.truncate(CATALOG_LIMIT);
    Ok(keys)
}

pub async fn objects(conn: &dyn Connection, schema: &str) -> Result<Vec<ObjectMeta>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT name, type, CASE WHEN type = 'trigger' THEN tbl_name END
             FROM {}.sqlite_master
             WHERE type IN ('table', 'view', 'trigger') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
             ORDER BY name",
            ident(schema)
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let kind = match r.text(1).as_str() {
                "view" => ObjectKind::View,
                "trigger" => ObjectKind::Trigger,
                _ => ObjectKind::Table,
            };
            ObjectMeta {
                parent: r.opt(2),
                ..ObjectMeta::new(r.text(0), kind, None)
            }
        })
        .collect())
}

/// The stored `CREATE` statement; for a table followed by its indexes and triggers.
pub async fn object_ddl(conn: &dyn Connection, o: &ObjectRef<'_>) -> Result<String> {
    let kind = match o.kind {
        ObjectKind::Table => "table",
        ObjectKind::View => "view",
        ObjectKind::Trigger => "trigger",
        other => return Err(unsupported(&format!("{other:?} objects in SQLite"))),
    };
    let master = format!("{}.sqlite_master", ident(o.schema));
    let mut sql = format!(
        "SELECT sql FROM {master} WHERE type = {} AND name = {}",
        literal(kind),
        literal(o.name)
    );
    if o.kind == ObjectKind::Table {
        // Automatic indexes (for UNIQUE and PRIMARY KEY) have no SQL.
        let _ = write!(
            sql,
            " UNION ALL SELECT sql FROM (SELECT sql FROM {master}
               WHERE type IN ('index', 'trigger') AND tbl_name = {} AND sql IS NOT NULL
               ORDER BY type, name)",
            literal(o.name)
        );
    }
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

    let constraints = key_constraints(&indexes);
    let referenced_by = referenced_by(conn, schema, table).await?;

    Ok(TableDetails {
        columns,
        indexes,
        foreign_keys,
        constraints,
        referenced_by,
    })
}

/// Primary and unique keys. SQLite keeps CHECK constraints only in the table's SQL.
fn key_constraints(indexes: &[IndexMeta]) -> Vec<ConstraintMeta> {
    let quoted = |cols: &[String]| {
        cols.iter()
            .map(|c| Dialect::Sqlite.quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ")
    };
    indexes
        .iter()
        .filter(|i| i.primary || i.unique)
        .map(|i| {
            let (kind, keyword) = if i.primary {
                (ConstraintKind::PrimaryKey, "PRIMARY KEY")
            } else {
                (ConstraintKind::Unique, "UNIQUE")
            };
            ConstraintMeta {
                name: i.name.clone(),
                kind,
                columns: i.columns.clone(),
                definition: format!("{keyword} ({})", quoted(&i.columns)),
            }
        })
        .collect()
}

/// Foreign keys of the other tables of `schema` that point at `table`.
async fn referenced_by(
    conn: &dyn Connection,
    schema: &str,
    table: &str,
) -> Result<Vec<ReferenceMeta>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT m.name, f.id, f.\"from\", coalesce(f.\"to\", '')
             FROM {}.sqlite_master m JOIN pragma_foreign_key_list(m.name, {}) f
             WHERE m.type = 'table' AND f.\"table\" = {} COLLATE NOCASE
             ORDER BY m.name, f.id, f.seq",
            ident(schema),
            literal(schema),
            literal(table)
        ),
    )
    .await?;
    Ok(group_sorted(
        rows.iter().map(|r| {
            (
                format!("{}{}", r.text(0), r.text(1)),
                ReferenceMeta {
                    name: format!("fk_{}_{}", r.text(0), r.text(1)),
                    schema: schema.to_owned(),
                    table: r.text(0),
                    columns: vec![r.text(2)],
                    ref_columns: vec![r.text(3)],
                },
            )
        }),
        |reference, row| {
            reference.columns.extend(row.columns);
            reference.ref_columns.extend(row.ref_columns);
        },
    ))
}
