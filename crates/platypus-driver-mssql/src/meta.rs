//! Navigator metadata from the catalog views (`sys.*`) of the session's database.

use platypus_core::{
    CATALOG_LIMIT, CatalogColumn, CatalogForeignKey, ColumnMeta, ConstraintKind, ConstraintMeta,
    Error, ForeignKeyMeta, IndexMeta, MatchKind, MetadataMatch, ObjectKind, ObjectMeta, ObjectRef,
    Property, ReferencingKey, Result, SEARCH_LIMIT, SchemaMeta, TableDetails, TextRow, TextRowExt,
    group_sorted, query_catalog, query_text, unsupported,
};
use platypus_sql::{Dialect, contains_pattern};

use crate::MssqlConnection;
use crate::ddl::{self, Check, Column, ForeignKey, Index, Sequence};

const D: Dialect = Dialect::SqlServer;

/// The `sys.objects` types the navigator shows.
const KINDS: &str = "'U', 'V', 'SO', 'FN', 'IF', 'TF', 'FS', 'FT', 'P', 'PC', 'TR', 'TA', 'SN'";

/// Groups rows of one constraint or index; names may contain any character.
const SEPARATOR: char = '\u{1f}';

fn lit(value: &str) -> String {
    D.quote_literal(value)
}

/// The `OBJECT_ID` of `schema.name`: NULL, which matches nothing, when there is none.
fn object_id(schema: &str, name: &str) -> String {
    format!("OBJECT_ID({})", lit(&D.qualified(Some(schema), name)))
}

/// A `sys.objects` type code, or `TY` for user-defined types.
fn object_kind(code: &str) -> ObjectKind {
    match code.trim() {
        "V" => ObjectKind::View,
        "SO" => ObjectKind::Sequence,
        "FN" | "IF" | "TF" | "FS" | "FT" | "AF" => ObjectKind::Function,
        "P" | "PC" | "X" => ObjectKind::Procedure,
        "TR" | "TA" => ObjectKind::Trigger,
        "TY" => ObjectKind::Type,
        "SN" => ObjectKind::Synonym,
        _ => ObjectKind::Table,
    }
}

pub async fn schemas(conn: &MssqlConnection) -> Result<Vec<SchemaMeta>> {
    // Schemas from 16384 up belong to the fixed database roles (`db_owner`, ...).
    let rows = query_text(
        conn,
        "SELECT name,
                CASE WHEN name IN ('sys', 'INFORMATION_SCHEMA', 'guest') OR schema_id >= 16384
                     THEN 1 ELSE 0 END,
                CASE WHEN name = SCHEMA_NAME() THEN 1 ELSE 0 END
         FROM sys.schemas
         ORDER BY name",
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

pub async fn databases(conn: &MssqlConnection) -> Result<Vec<String>> {
    let rows = query_text(
        conn,
        "SELECT name FROM sys.databases
         WHERE state = 0 AND HAS_DBACCESS(name) = 1
         ORDER BY name",
    )
    .await?;
    Ok(rows.iter().map(|r| r.text(0)).collect())
}

pub async fn catalog_columns(conn: &MssqlConnection) -> Result<Vec<CatalogColumn>> {
    query_catalog(
        conn,
        &format!(
            "SELECT TOP ({CATALOG_LIMIT}) s.name, o.name, c.name
             FROM sys.columns c
             JOIN sys.objects o ON o.object_id = c.object_id
             JOIN sys.schemas s ON s.schema_id = o.schema_id
             WHERE o.type IN ('U', 'V') AND o.is_ms_shipped = 0
             ORDER BY s.name, o.name, c.column_id"
        ),
    )
    .await
}

/// The foreign keys matching `filter` (on `k`, `sys.foreign_key_columns`), at most `top`.
async fn foreign_keys(conn: &MssqlConnection, top: &str, filter: &str) -> Result<Vec<ForeignKey>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT {top} ps.name, pt.name, fk.name, pc.name, rs.name, rt.name, rc.name,
                    fk.delete_referential_action_desc, fk.update_referential_action_desc
             FROM sys.foreign_key_columns k
             JOIN sys.foreign_keys fk ON fk.object_id = k.constraint_object_id
             JOIN sys.objects pt ON pt.object_id = k.parent_object_id
             JOIN sys.schemas ps ON ps.schema_id = pt.schema_id
             JOIN sys.columns pc
               ON pc.object_id = k.parent_object_id AND pc.column_id = k.parent_column_id
             JOIN sys.objects rt ON rt.object_id = k.referenced_object_id
             JOIN sys.schemas rs ON rs.schema_id = rt.schema_id
             JOIN sys.columns rc
               ON rc.object_id = k.referenced_object_id AND rc.column_id = k.referenced_column_id
             WHERE {filter}
             ORDER BY ps.name, pt.name, fk.name, k.constraint_column_id"
        ),
    )
    .await?;
    Ok(group_sorted(
        rows.iter().map(|r| {
            (
                [r.text(0), r.text(1), r.text(2)].join(&SEPARATOR.to_string()),
                ForeignKey {
                    schema: r.text(0),
                    table: r.text(1),
                    name: r.text(2),
                    columns: vec![r.text(3)],
                    ref_schema: r.text(4),
                    ref_table: r.text(5),
                    ref_columns: vec![r.text(6)],
                    on_delete: r.text(7),
                    on_update: r.text(8),
                },
            )
        }),
        |fk, row| {
            fk.columns.extend(row.columns);
            fk.ref_columns.extend(row.ref_columns);
        },
    ))
}

/// The foreign keys of the object `id`.
async fn own_foreign_keys(conn: &MssqlConnection, id: &str) -> Result<Vec<ForeignKey>> {
    foreign_keys(conn, "", &format!("k.parent_object_id = {id}")).await
}

pub async fn catalog_foreign_keys(conn: &MssqlConnection) -> Result<Vec<CatalogForeignKey>> {
    let keys = foreign_keys(
        conn,
        &format!("TOP ({CATALOG_LIMIT})"),
        "pt.is_ms_shipped = 0",
    )
    .await?;
    Ok(keys
        .into_iter()
        .map(|k| CatalogForeignKey {
            schema: k.schema,
            table: k.table,
            columns: k.columns,
            ref_schema: k.ref_schema,
            ref_table: k.ref_table,
            ref_columns: k.ref_columns,
        })
        .collect())
}

pub async fn referencing_keys(
    conn: &MssqlConnection,
    schema: &str,
    table: &str,
) -> Result<Vec<ReferencingKey>> {
    let filter = format!("k.referenced_object_id = {}", object_id(schema, table));
    Ok(foreign_keys(conn, "", &filter)
        .await?
        .into_iter()
        .map(|k| ReferencingKey {
            schema: k.schema,
            table: k.table,
            name: k.name,
            columns: k.columns,
            ref_columns: k.ref_columns,
        })
        .collect())
}

pub async fn objects(conn: &MssqlConnection, schema: &str) -> Result<Vec<ObjectMeta>> {
    let schema = lit(schema);
    let rows = query_text(
        conn,
        &format!(
            "SELECT o.name, o.type, CAST(ep.value AS NVARCHAR(4000)), p.name, r.n, sz.bytes
             FROM sys.objects o
             JOIN sys.schemas s ON s.schema_id = o.schema_id
             LEFT JOIN sys.objects p
               ON p.object_id = o.parent_object_id AND o.type IN ('TR', 'TA')
             LEFT JOIN sys.extended_properties ep
               ON ep.class = 1 AND ep.major_id = o.object_id AND ep.minor_id = 0
              AND ep.name = 'MS_Description'
             OUTER APPLY (
                 SELECT SUM(pt.rows) AS n FROM sys.partitions pt
                 WHERE pt.object_id = o.object_id AND pt.index_id IN (0, 1) AND o.type = 'U'
             ) r
             OUTER APPLY (
                 SELECT SUM(a.total_pages) * 8192 AS bytes
                 FROM sys.partitions pt
                 JOIN sys.allocation_units a ON a.container_id = pt.partition_id
                 WHERE pt.object_id = o.object_id AND o.type = 'U'
             ) sz
             WHERE s.name = {schema} AND o.is_ms_shipped = 0 AND o.type IN ({KINDS})
             UNION ALL
             SELECT t.name, 'TY', CAST(NULL AS NVARCHAR(4000)), CAST(NULL AS sysname), NULL, NULL
             FROM sys.types t JOIN sys.schemas s ON s.schema_id = t.schema_id
             WHERE t.is_user_defined = 1 AND s.name = {schema}
             ORDER BY 1"
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| ObjectMeta {
            parent: r.opt(3),
            rows: r.number(4),
            size: r.number(5),
            ..ObjectMeta::new(r.text(0), object_kind(&r.text(1)), r.opt(2))
        })
        .collect())
}

pub async fn search_metadata(conn: &MssqlConnection, needle: &str) -> Result<Vec<MetadataMatch>> {
    let pattern = lit(&contains_pattern(needle));
    let has = |expr: &str| D.contains_quoted(expr, &pattern);
    let (name, column, source) = (has("o.name"), has("c.name"), has("m.definition"));
    let rows = query_text(
        conn,
        &format!(
            "SELECT TOP ({SEARCH_LIMIT}) sch, obj, kind, parent, matched, col FROM (
                 SELECT s.name AS sch, o.name AS obj, o.type AS kind, p.name AS parent,
                        'name' AS matched, CAST(NULL AS sysname) AS col
                 FROM sys.objects o
                 JOIN sys.schemas s ON s.schema_id = o.schema_id
                 LEFT JOIN sys.objects p
                   ON p.object_id = o.parent_object_id AND o.type IN ('TR', 'TA')
                 WHERE o.is_ms_shipped = 0 AND o.type IN ({KINDS}) AND {name}
                 UNION ALL
                 SELECT s.name, o.name, o.type, NULL, 'column', c.name
                 FROM sys.columns c
                 JOIN sys.objects o ON o.object_id = c.object_id
                 JOIN sys.schemas s ON s.schema_id = o.schema_id
                 WHERE o.is_ms_shipped = 0 AND o.type IN ('U', 'V') AND {column}
                 UNION ALL
                 SELECT s.name, o.name, o.type, p.name, 'source', NULL
                 FROM sys.sql_modules m
                 JOIN sys.objects o ON o.object_id = m.object_id
                 JOIN sys.schemas s ON s.schema_id = o.schema_id
                 LEFT JOIN sys.objects p
                   ON p.object_id = o.parent_object_id AND o.type IN ('TR', 'TA')
                 WHERE o.is_ms_shipped = 0 AND o.type IN ({KINDS}) AND NOT {name} AND {source}
             ) hits
             ORDER BY sch, obj, col"
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| MetadataMatch {
            schema: r.text(0),
            name: r.text(1),
            kind: object_kind(&r.text(2)),
            parent: r.opt(3),
            matched: MatchKind::from_code(&r.text(4)),
            column: r.opt(5),
        })
        .collect())
}

/// The declared type in the four cells from `at`: the type's name, `max_length`,
/// `precision` and `scale`, as `sys.columns` and `sys.types` have them.
fn declared_type(r: &TextRow, at: usize, user_defined: bool) -> String {
    let byte = |i| {
        r.number(at + i)
            .and_then(|n| u8::try_from(n).ok())
            .unwrap_or(0)
    };
    ddl::declared_type(
        &r.text(at),
        r.number(at + 1),
        byte(2),
        byte(3),
        user_defined,
    )
}

/// The columns of the object `id` (an `OBJECT_ID` expression), in order.
async fn columns(conn: &MssqlConnection, id: &str) -> Result<Vec<Column>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT c.name, t.name, c.max_length, c.precision, c.scale, t.is_user_defined,
                    c.is_nullable, dc.definition, CAST(ep.value AS NVARCHAR(4000)),
                    c.is_identity, CAST(ic.seed_value AS NVARCHAR(40)),
                    CAST(ic.increment_value AS NVARCHAR(40)), cc.definition, cc.is_persisted,
                    CASE WHEN c.collation_name <> CONVERT(sysname, DATABASEPROPERTYEX(DB_NAME(), 'Collation'))
                         THEN c.collation_name END,
                    c.is_sparse, CASE WHEN dc.is_system_named = 0 THEN dc.name END
             FROM sys.columns c
             JOIN sys.types t ON t.user_type_id = c.user_type_id
             LEFT JOIN sys.default_constraints dc ON dc.object_id = c.default_object_id
             LEFT JOIN sys.identity_columns ic
               ON ic.object_id = c.object_id AND ic.column_id = c.column_id
             LEFT JOIN sys.computed_columns cc
               ON cc.object_id = c.object_id AND cc.column_id = c.column_id
             LEFT JOIN sys.extended_properties ep
               ON ep.class = 1 AND ep.major_id = c.object_id AND ep.minor_id = c.column_id
              AND ep.name = 'MS_Description'
             WHERE c.object_id = {id}
             ORDER BY c.column_id"
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| Column {
            name: r.text(0),
            data_type: declared_type(r, 1, r.flag(5)),
            nullable: r.flag(6),
            default: r.opt(7),
            comment: r.opt(8),
            identity: r.flag(9).then(|| (r.text(10), r.text(11))),
            computed: r.opt(12).map(|expression| (expression, r.flag(13))),
            collation: r.opt(14),
            sparse: r.flag(15),
            default_name: r.opt(16),
        })
        .collect())
}

/// The indexes of the object `id`, the primary key's first, with their size.
async fn indexes(conn: &MssqlConnection, id: &str) -> Result<Vec<Index>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT i.name, i.is_unique, i.is_primary_key, i.is_unique_constraint, i.type_desc,
                    COL_NAME(ic.object_id, ic.column_id), ic.is_descending_key,
                    ic.is_included_column, i.filter_definition, sz.bytes
             FROM sys.indexes i
             JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id
             OUTER APPLY (
                 SELECT SUM(a.total_pages) * 8192 AS bytes
                 FROM sys.partitions p
                 JOIN sys.allocation_units a ON a.container_id = p.partition_id
                 WHERE p.object_id = i.object_id AND p.index_id = i.index_id
             ) sz
             WHERE i.object_id = {id} AND i.is_hypothetical = 0
             ORDER BY i.is_primary_key DESC, i.name, ic.is_included_column, ic.key_ordinal,
                      ic.index_column_id"
        ),
    )
    .await?;
    Ok(group_sorted(
        rows.iter().map(|r| {
            let column = r.text(5);
            let (columns, included) = if r.flag(7) {
                (Vec::new(), vec![column])
            } else {
                (vec![(column, r.flag(6))], Vec::new())
            };
            (
                r.text(0),
                Index {
                    name: r.text(0),
                    unique: r.flag(1),
                    primary: r.flag(2),
                    unique_constraint: r.flag(3),
                    kind: r.text(4),
                    columns,
                    included,
                    filter: r.opt(8),
                    size: r.number(9),
                },
            )
        }),
        |index, row| {
            index.columns.extend(row.columns);
            index.included.extend(row.included);
        },
    ))
}

async fn checks(conn: &MssqlConnection, id: &str) -> Result<Vec<Check>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT name, COL_NAME(parent_object_id, NULLIF(parent_column_id, 0)), definition
             FROM sys.check_constraints
             WHERE parent_object_id = {id}
             ORDER BY name"
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| Check {
            name: r.text(0),
            column: r.opt(1),
            definition: r.text(2),
        })
        .collect())
}

/// Primary key and unique constraints (from their indexes), then checks.
fn constraints(indexes: &[Index], checks: Vec<Check>) -> Vec<ConstraintMeta> {
    let keys = indexes.iter().filter(|i| i.is_constraint()).map(|i| {
        let columns: Vec<String> = i.columns.iter().map(|(c, _)| c.clone()).collect();
        let (kind, keyword) = if i.primary {
            (ConstraintKind::PrimaryKey, "PRIMARY KEY")
        } else {
            (ConstraintKind::Unique, "UNIQUE")
        };
        ConstraintMeta {
            name: i.name.clone(),
            kind,
            definition: format!("{keyword} ({})", ddl::names(&columns)),
            columns,
        }
    });
    let checks = checks.into_iter().map(|c| ConstraintMeta {
        name: c.name,
        kind: ConstraintKind::Check,
        columns: c.column.into_iter().collect(),
        definition: format!("CHECK {}", c.definition),
    });
    keys.chain(checks).collect()
}

pub async fn table_details(
    conn: &MssqlConnection,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let id = object_id(schema, table);
    let columns = columns(conn, &id).await?;
    let indexes = indexes(conn, &id).await?;
    let checks = checks(conn, &id).await?;
    let foreign_keys = own_foreign_keys(conn, &id).await?;
    let referenced_by = referencing_keys(conn, schema, table).await?;

    let primary: Vec<&str> = indexes
        .iter()
        .filter(|i| i.primary)
        .flat_map(|i| i.columns.iter().map(|(c, _)| c.as_str()))
        .collect();
    let columns = columns
        .into_iter()
        .map(|c| ColumnMeta {
            primary_key: primary.contains(&c.name.as_str()),
            name: c.name,
            data_type: c.data_type,
            nullable: c.nullable,
            default: c.default,
            comment: c.comment,
            enum_values: Vec::new(),
            fields: Vec::new(),
            hidden: false,
        })
        .collect();
    let constraints = constraints(&indexes, checks);
    Ok(TableDetails {
        columns,
        indexes: indexes
            .into_iter()
            .map(|i| IndexMeta {
                name: i.name,
                columns: i.columns.into_iter().map(|(c, _)| c).collect(),
                unique: i.unique,
                primary: i.primary,
                partial: i.filter.is_some(),
                size: i.size,
                detail: None,
            })
            .collect(),
        foreign_keys: foreign_keys
            .into_iter()
            .map(|k| ForeignKeyMeta {
                name: k.name,
                columns: k.columns,
                ref_schema: k.ref_schema,
                ref_table: k.ref_table,
                ref_columns: k.ref_columns,
                detail: None,
            })
            .collect(),
        constraints,
        referenced_by,
    })
}

/// The first row of `sql`, or an error naming `what` when there is none.
async fn first_row(conn: &MssqlConnection, sql: &str, what: &str) -> Result<TextRow> {
    query_text(conn, sql)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| Error::Other(format!("no such object: {what}")))
}

async fn sequence_ddl(conn: &MssqlConnection, qualified: &str, id: &str) -> Result<String> {
    let r = first_row(
        conn,
        &format!(
            "SELECT TYPE_NAME(user_type_id), CAST(start_value AS NVARCHAR(40)),
                    CAST(increment AS NVARCHAR(40)), CAST(minimum_value AS NVARCHAR(40)),
                    CAST(maximum_value AS NVARCHAR(40)), is_cycling
             FROM sys.sequences WHERE object_id = {id}"
        ),
        qualified,
    )
    .await?;
    let s = Sequence {
        data_type: r.text(0),
        start: r.text(1),
        increment: r.text(2),
        min: r.text(3),
        max: r.text(4),
        cycle: r.flag(5),
    };
    Ok(ddl::sequence(qualified, &s))
}

/// Alias types; table and CLR types have no definition to rebuild here.
async fn type_ddl(conn: &MssqlConnection, qualified: &str) -> Result<String> {
    let r = first_row(
        conn,
        &format!(
            "SELECT TYPE_NAME(system_type_id), max_length, precision, scale, is_nullable,
                    CASE WHEN is_table_type = 1 OR is_assembly_type = 1 THEN 1 ELSE 0 END
             FROM sys.types WHERE user_type_id = TYPE_ID({})",
            lit(qualified)
        ),
        qualified,
    )
    .await?;
    if r.flag(5) {
        return Err(unsupported("definitions of table and CLR types"));
    }
    let base = declared_type(&r, 0, false);
    let null = if r.flag(4) { "" } else { " NOT NULL" };
    Ok(format!("CREATE TYPE {qualified} FROM {base}{null};"))
}

pub async fn object_ddl(conn: &MssqlConnection, o: &ObjectRef<'_>) -> Result<String> {
    let qualified = D.qualified(Some(o.schema), o.name);
    let id = object_id(o.schema, o.name);
    match o.kind {
        ObjectKind::Table => {
            let columns = columns(conn, &id).await?;
            // A table has at least one column: none means there is no such table.
            if columns.is_empty() {
                return Err(Error::Other(format!("no such object: {qualified}")));
            }
            let mut script = ddl::table(
                &qualified,
                &columns,
                &indexes(conn, &id).await?,
                &checks(conn, &id).await?,
                &own_foreign_keys(conn, &id).await?,
            );
            let table_comment = first_row(
                conn,
                &format!(
                    "SELECT CAST(ep.value AS NVARCHAR(4000)) FROM sys.extended_properties ep
                     WHERE ep.class = 1 AND ep.major_id = {id} AND ep.minor_id = 0
                       AND ep.name = 'MS_Description'"
                ),
                &qualified,
            )
            .await
            .ok()
            .and_then(|r| r.opt(0));
            let comments = ddl::comments(o.schema, o.name, table_comment.as_deref(), &columns);
            if !comments.is_empty() {
                script.push_str("\n\n");
                script.push_str(&comments);
            }
            Ok(script)
        }
        ObjectKind::View | ObjectKind::Function | ObjectKind::Procedure | ObjectKind::Trigger => {
            first_row(conn, &format!("SELECT OBJECT_DEFINITION({id})"), &qualified)
                .await?
                .opt(0)
                .map(|definition| definition.trim().to_owned())
                .ok_or_else(|| {
                    Error::Other(format!(
                        "the definition of {qualified} is encrypted or not visible to this login"
                    ))
                })
        }
        ObjectKind::Sequence => sequence_ddl(conn, &qualified, &id).await,
        ObjectKind::Type => type_ddl(conn, &qualified).await,
        ObjectKind::Synonym => {
            let sql = format!("SELECT base_object_name FROM sys.synonyms WHERE object_id = {id}");
            let target = first_row(conn, &sql, &qualified).await?.text(0);
            Ok(format!("CREATE SYNONYM {qualified} FOR {target};"))
        }
        other => Err(unsupported(&format!("{other:?} objects in SQL Server"))),
    }
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

/// Bytes on disk of the object's partitions whose index matches `indexes`.
fn pages(indexes: &str) -> String {
    format!(
        "(SELECT SUM(a.total_pages) * 8192 FROM sys.partitions p
          JOIN sys.allocation_units a ON a.container_id = p.partition_id
          WHERE p.object_id = o.object_id AND p.index_id {indexes})"
    )
}

pub async fn object_properties(conn: &MssqlConnection, o: &ObjectRef<'_>) -> Result<Vec<Property>> {
    let id = object_id(o.schema, o.name);
    let common = "USER_NAME(OBJECTPROPERTY(o.object_id, 'OwnerId')),
                  CONVERT(VARCHAR(19), o.create_date, 120), CONVERT(VARCHAR(19), o.modify_date, 120)";
    let (sql, labels): (String, &[&str]) = match o.kind {
        ObjectKind::Table => (
            format!(
                "SELECT (SELECT SUM(p.rows) FROM sys.partitions p
                         WHERE p.object_id = o.object_id AND p.index_id IN (0, 1)),
                        {}, {}, {common}
                 FROM sys.objects o WHERE o.object_id = {id}",
                pages("IN (0, 1)"),
                pages("> 1")
            ),
            &[
                "Estimated rows",
                "Data size",
                "Indexes size",
                "Owner",
                "Created",
                "Modified",
            ],
        ),
        ObjectKind::View
        | ObjectKind::Function
        | ObjectKind::Procedure
        | ObjectKind::Trigger
        | ObjectKind::Sequence
        | ObjectKind::Synonym => (
            format!("SELECT o.type_desc, {common} FROM sys.objects o WHERE o.object_id = {id}"),
            &["Type", "Owner", "Created", "Modified"],
        ),
        _ => return Ok(Vec::new()),
    };
    let Some(r) = query_text(conn, &sql).await?.into_iter().next() else {
        return Ok(Vec::new());
    };
    Ok(labels
        .iter()
        .enumerate()
        .filter_map(|(i, label)| {
            let value = r.opt(i)?;
            let value = if label.ends_with("size") {
                size(&value)
            } else {
                value
            };
            Some(Property::new(*label, value))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_object_types() {
        assert_eq!(object_kind("U "), ObjectKind::Table);
        assert_eq!(object_kind("V "), ObjectKind::View);
        assert_eq!(object_kind("IF"), ObjectKind::Function);
        assert_eq!(object_kind("P "), ObjectKind::Procedure);
        assert_eq!(object_kind("TR"), ObjectKind::Trigger);
        assert_eq!(object_kind("SO"), ObjectKind::Sequence);
        assert_eq!(object_kind("TY"), ObjectKind::Type);
        assert_eq!(object_kind("SN"), ObjectKind::Synonym);
    }

    #[test]
    fn names_objects_for_the_catalog() {
        assert_eq!(object_id("dbo", "orders"), "OBJECT_ID(N'dbo.orders')");
        assert_eq!(
            object_id("my schema", "it's"),
            "OBJECT_ID(N'[my schema].[it''s]')"
        );
    }

    #[test]
    fn lists_constraints_from_indexes() {
        let indexes = [
            Index {
                name: "pk".to_owned(),
                primary: true,
                unique: true,
                columns: vec![("id".to_owned(), false)],
                ..Index::default()
            },
            Index {
                name: "uq".to_owned(),
                unique: true,
                unique_constraint: true,
                columns: vec![("order".to_owned(), false), ("b".to_owned(), true)],
                ..Index::default()
            },
            Index {
                name: "plain".to_owned(),
                unique: true,
                columns: vec![("c".to_owned(), false)],
                ..Index::default()
            },
        ];
        let checks = vec![Check {
            name: "ck".to_owned(),
            column: None,
            definition: "([a]>(0))".to_owned(),
        }];
        let found: Vec<_> = constraints(&indexes, checks)
            .into_iter()
            .map(|c| (c.kind, c.columns.join(","), c.definition))
            .collect();
        assert_eq!(
            found,
            [
                (
                    ConstraintKind::PrimaryKey,
                    "id".to_owned(),
                    "PRIMARY KEY (id)".to_owned()
                ),
                (
                    ConstraintKind::Unique,
                    "order,b".to_owned(),
                    "UNIQUE ([order], b)".to_owned()
                ),
                (
                    ConstraintKind::Check,
                    String::new(),
                    "CHECK ([a]>(0))".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(size("512"), "512 bytes");
        assert_eq!(size("1536"), "1.5 KB");
        assert_eq!(size("x"), "x");
    }
}
