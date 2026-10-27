//! Navigator metadata from `sqlite_master` and the `pragma_*` table-valued functions.

use std::fmt::Write as _;

use platypus_core::{
    CATALOG_LIMIT, CatalogColumn, CatalogForeignKey, ColumnMeta, Connection, ConstraintKind,
    ConstraintMeta, Error, Explain, ExplainFormat, ForeignKeyMeta, IndexMeta, MatchKind,
    MetadataMatch, ObjectKind, ObjectMeta, ObjectRef, PlanNode, ReferencingKey, Result,
    SEARCH_LIMIT, SchemaMeta, TableDetails, TextRowExt, group_sorted, query_catalog, query_text,
    unsupported,
};
use platypus_sql::{Dialect, Lexer, Token, TokenKind};

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
        .map(|r| ObjectMeta {
            parent: r.opt(2),
            ..ObjectMeta::new(r.text(0), object_kind(&r.text(1)), None)
        })
        .collect())
}

/// A `sqlite_master.type`.
fn object_kind(code: &str) -> ObjectKind {
    match code {
        "view" => ObjectKind::View,
        "trigger" => ObjectKind::Trigger,
        _ => ObjectKind::Table,
    }
}

pub async fn search_metadata(conn: &dyn Connection, needle: &str) -> Result<Vec<MetadataMatch>> {
    let has = |expr: &str| Dialect::Sqlite.contains(expr, needle);
    let (name, column, source) = (has("m.name"), has("p.name"), has("m.sql"));
    let user = r"m.name NOT LIKE 'sqlite\_%' ESCAPE '\'";
    let parent = "CASE WHEN m.type = 'trigger' THEN m.tbl_name END";
    let mut found = Vec::new();
    for schema in schemas(conn).await? {
        let (lit, id) = (literal(&schema.name), ident(&schema.name));
        let rows = query_text(
            conn,
            &format!(
                "SELECT m.name, m.type, {parent}, 'name', NULL
                 FROM {id}.sqlite_master m
                 WHERE {user} AND m.type IN ('table', 'view', 'trigger') AND {name}
                 UNION ALL
                 SELECT m.name, m.type, NULL, 'column', p.name
                 FROM {id}.sqlite_master m JOIN pragma_table_info(m.name, {lit}) p
                 WHERE {user} AND m.type IN ('table', 'view') AND {column}
                 UNION ALL
                 SELECT m.name, m.type, {parent}, 'source', NULL
                 FROM {id}.sqlite_master m
                 WHERE {user} AND m.type IN ('view', 'trigger') AND NOT {name} AND {source}
                 ORDER BY 1, 5
                 LIMIT {SEARCH_LIMIT}"
            ),
        )
        .await?;
        found.extend(rows.iter().map(|r| MetadataMatch {
            schema: schema.name.clone(),
            name: r.text(0),
            kind: object_kind(&r.text(1)),
            parent: r.opt(2),
            matched: MatchKind::from_code(&r.text(3)),
            column: r.opt(4),
        }));
    }
    found.truncate(SEARCH_LIMIT);
    Ok(found)
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

/// The columns of a table, from the literals of its schema and name, in table order.
async fn columns(
    conn: &dyn Connection,
    schema_lit: &str,
    table_lit: &str,
) -> Result<Vec<ColumnMeta>> {
    // `hidden` is 1 for hidden virtual-table columns, 2 and 3 for generated columns.
    Ok(query_text(
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
        fields: Vec::new(),
    })
    .collect())
}

pub async fn table_details(
    conn: &dyn Connection,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let (schema_lit, table_lit) = (literal(schema), literal(table));

    let columns = columns(conn, &schema_lit, &table_lit).await?;

    let rows = query_text(
        conn,
        &format!(
            "SELECT il.name, il.\"unique\", il.origin = 'pk', coalesce(ii.name, '(expression)'),
                    il.partial
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
                    partial: r.flag(4),
                    size: None,
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
                partial: false,
                size: None,
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

    let mut constraints = key_constraints(&indexes);
    let sql = query_text(
        conn,
        &format!(
            "SELECT sql FROM {}.sqlite_master WHERE type = 'table' AND name = {table_lit}",
            ident(schema)
        ),
    )
    .await?;
    if let Some(sql) = sql.first().and_then(|r| r.opt(0)) {
        constraints.extend(check_constraints(table, &sql));
    }
    let referenced_by = referencing_keys(conn, schema, table).await?;

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

/// The CHECK constraints of a table's `CREATE TABLE`, as SQLite keeps them nowhere else;
/// an unnamed one is numbered, column ones included.
fn check_constraints(table: &str, sql: &str) -> Vec<ConstraintMeta> {
    let tokens: Vec<Token> = Lexer::new(sql, Dialect::Sqlite)
        .filter(|t| !t.is_trivia())
        .collect();
    let text = |i: usize| tokens.get(i).map_or("", |t| t.text(sql));
    let word = |i: usize, w: &str| {
        tokens
            .get(i)
            .is_some_and(|t| t.kind == TokenKind::Word && t.text(sql).eq_ignore_ascii_case(w))
    };
    let mut checks = Vec::new();
    for i in 0..tokens.len() {
        if !word(i, "CHECK") || text(i + 1) != "(" {
            continue;
        }
        let mut depth = 0;
        let Some(end) = (i + 1..tokens.len()).find(|&j| {
            match text(j) {
                "(" => depth += 1,
                ")" => depth -= 1,
                _ => {}
            }
            depth == 0
        }) else {
            break;
        };
        let name = if i >= 2 && word(i - 2, "CONSTRAINT") {
            let quoted = tokens[i - 1].kind == TokenKind::QuotedIdent;
            let name = text(i - 1);
            if quoted {
                &name[1..name.len() - 1]
            } else {
                name
            }
            .to_owned()
        } else {
            format!("{table}_check{}", checks.len() + 1)
        };
        checks.push(ConstraintMeta {
            name,
            kind: ConstraintKind::Check,
            columns: Vec::new(),
            definition: sql[tokens[i].start..tokens[end].end].to_owned(),
        });
    }
    checks
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

/// `EXPLAIN QUERY PLAN` as a tree: its rows are `(id, parent, notused, detail)`.
pub async fn explain(conn: &dyn Connection, sql: &str) -> Result<Explain> {
    let rows = query_text(conn, &format!("EXPLAIN QUERY PLAN {sql}")).await?;
    let steps: Vec<_> = rows
        .iter()
        .map(|r| (r.number(0), r.number(1), r.text(3)))
        .collect();
    let mut plan = String::new();
    let root = PlanNode {
        children: plan_steps(&steps, Some(0), 0, &mut plan),
        ..PlanNode::new("QUERY PLAN")
    };
    Ok(Explain::new(
        ExplainFormat::Text,
        format!("QUERY PLAN\n{plan}"),
        root,
        false,
    ))
}

/// The steps under `parent`, also written to `text` indented by `depth`.
fn plan_steps(
    steps: &[(Option<u64>, Option<u64>, String)],
    parent: Option<u64>,
    depth: usize,
    text: &mut String,
) -> Vec<PlanNode> {
    steps
        .iter()
        .filter(|(_, p, _)| *p == parent)
        .map(|(id, _, detail)| {
            let _ = writeln!(text, "{}{detail}", "  ".repeat(depth + 1));
            PlanNode {
                children: plan_steps(steps, *id, depth + 1, text),
                ..PlanNode::new(detail.clone())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_checks_from_the_table_sql() {
        let sql = "CREATE TABLE t (qty INT CHECK (qty >= (0)), note TEXT DEFAULT 'CHECK (x)', \
                   CONSTRAINT \"positive\" CHECK (qty < 10))";
        let checks: Vec<_> = check_constraints("t", sql)
            .into_iter()
            .map(|c| (c.name, c.definition))
            .collect();
        assert_eq!(
            checks,
            [
                ("t_check1".to_owned(), "CHECK (qty >= (0))".to_owned()),
                ("positive".to_owned(), "CHECK (qty < 10)".to_owned()),
            ]
        );
    }

    #[test]
    fn nests_plan_steps_by_parent() {
        let steps = [
            (Some(2), Some(0), "SCAN a".to_owned()),
            (Some(5), Some(0), "CORRELATED SCALAR SUBQUERY 1".to_owned()),
            (Some(9), Some(5), "SEARCH b USING INDEX i (x=?)".to_owned()),
        ];
        let mut text = String::new();
        let top = plan_steps(&steps, Some(0), 0, &mut text);
        assert_eq!(top.len(), 2);
        assert_eq!(top[1].children[0].operation, "SEARCH b USING INDEX i (x=?)");
        assert_eq!(
            text,
            "  SCAN a\n  CORRELATED SCALAR SUBQUERY 1\n    SEARCH b USING INDEX i (x=?)\n"
        );
    }
}
