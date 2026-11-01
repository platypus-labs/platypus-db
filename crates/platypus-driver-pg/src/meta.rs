//! Navigator metadata from `pg_catalog`.

use platypus_core::{
    CATALOG_LIMIT, CatalogColumn, CatalogForeignKey, ColumnMeta, Connection, ConstraintKind,
    ConstraintMeta, Error, FieldMeta, ForeignKeyMeta, IndexMeta, MatchKind, MetadataMatch,
    ObjectKind, ObjectMeta, ReferencingKey, Result, SEARCH_LIMIT, SchemaMeta, TableDetails,
    TextRow, TextRowExt, query_catalog, query_text,
};
use platypus_sql::Dialect;

/// Separates list items aggregated into one text cell.
const SEP: char = '\u{1f}';

/// A string literal that is valid whatever `standard_conforming_strings` says.
/// The SQL of the type of column `a` (a `pg_attribute`) as `format_type` writes it, except
/// CockroachDB's `STRING(n)`: it is `text` with a length there, which its `format_type`
/// writes `text(n + 4)`, a type neither database takes. PostgreSQL's `text` has none.
pub(crate) const COLUMN_TYPE: &str = "CASE WHEN a.atttypid IN (25, 1009) AND a.atttypmod > 4 \
     THEN 'character varying(' || (a.atttypmod - 4)::text || ')' \
          || CASE WHEN a.atttypid = 1009 THEN '[]' ELSE '' END \
     ELSE format_type(a.atttypid, a.atttypmod) END";

pub(crate) fn literal(s: &str) -> String {
    format!("E'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
}

pub(crate) fn list(cell: Option<String>) -> Vec<String> {
    cell.filter(|s| !s.is_empty())
        .map(|s| s.split(SEP).map(str::to_owned).collect())
        .unwrap_or_default()
}

pub async fn schemas(conn: &dyn Connection) -> Result<Vec<SchemaMeta>> {
    let rows = query_text(
        conn,
        // CockroachDB adds its own catalog schemas; an extension may bring schemas of its
        // own (TimescaleDB's `_timescaledb_catalog`, `timescaledb_information`…).
        "SELECT n.nspname,
                n.nspname IN ('pg_catalog', 'information_schema', 'crdb_internal', 'pg_extension')
                OR EXISTS (SELECT 1 FROM pg_depend d
                           WHERE d.classid = 'pg_namespace'::regclass AND d.objid = n.oid
                             AND d.deptype = 'e'),
                n.nspname = current_schema()
         FROM pg_namespace n
         WHERE n.nspname NOT LIKE 'pg\\_toast%' AND n.nspname NOT LIKE 'pg\\_temp\\_%'
         ORDER BY n.nspname",
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

pub async fn catalog_columns(conn: &dyn Connection) -> Result<Vec<CatalogColumn>> {
    query_catalog(
        conn,
        &format!(
            "SELECT n.nspname, c.relname, a.attname
             FROM pg_class c
             JOIN pg_namespace n ON n.oid = c.relnamespace
             JOIN pg_attribute a ON a.attrelid = c.oid
             WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND a.attnum > 0 AND NOT a.attisdropped
               AND n.nspname NOT IN ('pg_catalog', 'information_schema')
               AND n.nspname NOT LIKE 'pg\\_toast%' AND n.nspname NOT LIKE 'pg\\_temp\\_%'
             ORDER BY 1, 2, a.attnum
             LIMIT {CATALOG_LIMIT}"
        ),
    )
    .await
}

pub async fn catalog_foreign_keys(conn: &dyn Connection) -> Result<Vec<CatalogForeignKey>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT n.nspname, c.relname,
                    array_to_string(ARRAY(
                        SELECT a.attname FROM unnest(con.conkey) WITH ORDINALITY k(n, o)
                        JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.n
                        ORDER BY k.o), chr(31)),
                    fn.nspname, fc.relname,
                    array_to_string(ARRAY(
                        SELECT a.attname FROM unnest(con.confkey) WITH ORDINALITY k(n, o)
                        JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.n
                        ORDER BY k.o), chr(31))
             FROM pg_constraint con
             JOIN pg_class c ON c.oid = con.conrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             JOIN pg_class fc ON fc.oid = con.confrelid
             JOIN pg_namespace fn ON fn.oid = fc.relnamespace
             WHERE con.contype = 'f'
               AND n.nspname NOT IN ('pg_catalog', 'information_schema')
             ORDER BY 1, 2, con.conname
             LIMIT {CATALOG_LIMIT}"
        ),
    )
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| CatalogForeignKey {
            schema: r.text(0),
            table: r.text(1),
            columns: list(r.opt(2)),
            ref_schema: r.text(3),
            ref_table: r.text(4),
            ref_columns: list(r.opt(5)),
        })
        .collect())
}

/// The hidden key of the table `c` (a `pg_class` row): the `rowid` CockroachDB adds, with
/// `unique_rowid()` as its default, when the table declares no primary key. It is the primary
/// index then, and `SELECT *` leaves it out.
const HIDDEN_ROWID: &str = "(SELECT a.attname
        FROM pg_index i
        JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = i.indkey[0]
        JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
        WHERE i.indrelid = c.oid AND i.indisprimary AND i.indnkeyatts = 1
          AND a.attname = 'rowid' AND pg_get_expr(d.adbin, d.adrelid) LIKE 'unique_rowid()%')";

pub async fn objects(conn: &dyn Connection, schema: &str) -> Result<Vec<ObjectMeta>> {
    let schema = literal(schema);
    let hidden = if crate::is_cockroach(conn) {
        format!("CASE WHEN c.relkind = 'r' THEN {HIDDEN_ROWID} END")
    } else {
        "NULL".to_owned()
    };
    let rows = query_text(
        conn,
        &format!(
            "SELECT c.relname, c.relkind::text, obj_description(c.oid, 'pg_class'),
                    -- A partition shows under its table when both are in this schema.
                    CASE WHEN c.relispartition THEN
                        (SELECT pc.relname FROM pg_inherits i JOIN pg_class pc ON pc.oid = i.inhparent
                         WHERE i.inhrelid = c.oid AND pc.relnamespace = c.relnamespace) END,
                    -- -1 until the table is first analyzed.
                    CASE WHEN c.relkind IN ('r', 'm') AND c.reltuples >= 0 THEN c.reltuples::bigint END,
                    -- From the statistics: pg_total_relation_size would lock every table.
                    -- Each part by its own index lookup, so the query stays linear.
                    CASE WHEN c.relkind IN ('r', 'm') THEN
                        (c.relpages::bigint
                         + COALESCE((SELECT t.relpages FROM pg_class t WHERE t.oid = c.reltoastrelid), 0)
                         + COALESCE((SELECT sum(ic.relpages) FROM pg_index i
                                     JOIN pg_class ic ON ic.oid = i.indexrelid
                                     WHERE i.indrelid = c.oid), 0))
                        * current_setting('block_size')::bigint END,
                    {hidden}
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = {schema} AND c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')
             UNION ALL
             SELECT p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')',
                    CASE p.prokind WHEN 'p' THEN 'P' ELSE 'F' END,
                    obj_description(p.oid, 'pg_proc'), NULL, NULL, NULL, NULL
             FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
             WHERE n.nspname = {schema} AND p.prokind IN ('f', 'p')
               -- Not the routines an extension brings (TimescaleDB's, in public).
               AND NOT EXISTS (SELECT 1 FROM pg_depend d
                               WHERE d.classid = 'pg_proc'::regclass AND d.objid = p.oid AND d.deptype = 'e')
             UNION ALL
             SELECT t.tgname, 'T', obj_description(t.oid, 'pg_trigger'), c.relname, NULL, NULL, NULL
             FROM pg_trigger t
             JOIN pg_class c ON c.oid = t.tgrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = {schema} AND NOT t.tgisinternal
             UNION ALL
             -- Enums, domains, ranges and standalone composite types; not the row types
             -- of tables, nor the types an extension brings.
             SELECT t.typname, CASE t.typtype WHEN 'd' THEN 'D' ELSE 'Y' END,
                    obj_description(t.oid, 'pg_type'), NULL, NULL, NULL, NULL
             FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
             WHERE n.nspname = {schema}
               AND (t.typtype IN ('e', 'd', 'r')
                    OR (t.typtype = 'c' AND (SELECT relkind FROM pg_class WHERE oid = t.typrelid) = 'c'))
               AND NOT EXISTS (SELECT 1 FROM pg_depend d
                               WHERE d.classid = 'pg_type'::regclass AND d.objid = t.oid AND d.deptype = 'e')
             UNION ALL
             SELECT e.extname, 'E', obj_description(e.oid, 'pg_extension'), NULL, NULL, NULL, NULL
             FROM pg_extension e JOIN pg_namespace n ON n.oid = e.extnamespace
             WHERE n.nspname = {schema}
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
            hidden_key: r.opt(6),
            ..ObjectMeta::new(r.text(0), object_kind(&r.text(1)), r.opt(2))
        })
        .collect())
}

/// The `name\ttype` lines of the columns query's composite fields.
fn composite_fields(lines: &str) -> Vec<FieldMeta> {
    lines
        .split('\n')
        .filter_map(|line| line.split_once('\t'))
        .map(|(name, data_type)| FieldMeta {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
        })
        .collect()
}

/// The search runs as one catalog query; kind codes are those of [`objects`].
pub async fn search_metadata(conn: &dyn Connection, needle: &str) -> Result<Vec<MetadataMatch>> {
    let has = |expr: &str| Dialect::Postgres.contains(expr, needle);
    let (relname, attname, viewdef) = (
        has("c.relname"),
        has("a.attname"),
        has("pg_get_viewdef(c.oid)"),
    );
    let (proname, prosrc, tgname) = (has("p.proname"), has("p.prosrc"), has("t.tgname"));
    let rows = query_text(
        conn,
        &format!(
            "WITH n AS (SELECT oid, nspname FROM pg_namespace
                        WHERE nspname NOT IN ('pg_catalog', 'information_schema')
                          AND nspname NOT LIKE 'pg\\_toast%' AND nspname NOT LIKE 'pg\\_temp\\_%')
             SELECT n.nspname, c.relname, c.relkind::text, NULL, 'name', NULL
             FROM pg_class c JOIN n ON n.oid = c.relnamespace
             WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S') AND {relname}
             UNION ALL
             SELECT n.nspname, c.relname, c.relkind::text, NULL, 'column', a.attname
             FROM pg_class c JOIN n ON n.oid = c.relnamespace
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
             WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND {attname}
             UNION ALL
             SELECT n.nspname, c.relname, c.relkind::text, NULL, 'source', NULL
             FROM pg_class c JOIN n ON n.oid = c.relnamespace
             WHERE c.relkind IN ('v', 'm') AND NOT {relname} AND {viewdef}
             UNION ALL
             SELECT n.nspname, p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')',
                    CASE p.prokind WHEN 'p' THEN 'P' ELSE 'F' END, NULL,
                    CASE WHEN {proname} THEN 'name' ELSE 'source' END, NULL
             FROM pg_proc p JOIN n ON n.oid = p.pronamespace
             WHERE p.prokind IN ('f', 'p') AND ({proname} OR {prosrc})
             UNION ALL
             SELECT n.nspname, t.tgname, 'T', c.relname, 'name', NULL
             FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid JOIN n ON n.oid = c.relnamespace
             WHERE NOT t.tgisinternal AND {tgname}
             ORDER BY 1, 2, 6
             LIMIT {SEARCH_LIMIT}"
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

/// The kind codes of the `objects` query: `pg_class.relkind`, plus `F`/`P` for routines,
/// `T` for triggers, `Y` for types, `D` for domains and `E` for extensions.
fn object_kind(code: &str) -> ObjectKind {
    match code {
        "v" => ObjectKind::View,
        "m" => ObjectKind::MaterializedView,
        "f" => ObjectKind::ForeignTable,
        "S" => ObjectKind::Sequence,
        "F" => ObjectKind::Function,
        "P" => ObjectKind::Procedure,
        "T" => ObjectKind::Trigger,
        "Y" => ObjectKind::Type,
        "D" => ObjectKind::Domain,
        "E" => ObjectKind::Extension,
        _ => ObjectKind::Table,
    }
}

/// The columns of `relation` (an oid subquery), in table order.
async fn columns(conn: &dyn Connection, relation: &str) -> Result<Vec<ColumnMeta>> {
    Ok(query_text(
        conn,
        &format!(
            "SELECT a.attname, {COLUMN_TYPE}, NOT a.attnotnull,
                    pg_get_expr(d.adbin, d.adrelid),
                    coalesce(a.attnum = ANY (i.indkey), false),
                    col_description(a.attrelid, a.attnum),
                    (SELECT string_agg(e.enumlabel, E'\\n' ORDER BY e.enumsortorder)
                     FROM pg_enum e WHERE e.enumtypid = a.atttypid),
                    (SELECT string_agg(f.attname || E'\\t' || format_type(f.atttypid, f.atttypmod),
                                       E'\\n' ORDER BY f.attnum)
                     FROM pg_type t
                     JOIN pg_type c ON c.oid = CASE WHEN t.typelem <> 0 AND t.typlen = -1
                                                    THEN t.typelem ELSE t.oid END
                     JOIN pg_attribute f ON f.attrelid = c.typrelid
                     WHERE t.oid = a.atttypid AND c.typtype = 'c'
                       AND f.attnum > 0 AND NOT f.attisdropped)
             FROM pg_attribute a
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
             LEFT JOIN pg_index i ON i.indrelid = a.attrelid AND i.indisprimary
             WHERE a.attrelid = {relation} AND a.attnum > 0 AND NOT a.attisdropped
             ORDER BY a.attnum"
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
        comment: r.opt(5),
        enum_values: r
            .opt(6)
            .map(|labels| labels.split('\n').map(str::to_owned).collect())
            .unwrap_or_default(),
        fields: r.opt(7).map(|f| composite_fields(&f)).unwrap_or_default(),
    })
    .collect())
}

pub async fn table_details(
    conn: &dyn Connection,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let relation = format!(
        "(SELECT c.oid FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
          WHERE n.nspname = {} AND c.relname = {})",
        literal(schema),
        literal(table)
    );

    let columns = columns(conn, &relation).await?;
    // No columns: a table without any, or none at all (dropped since it was listed).
    if columns.is_empty()
        && query_text(conn, &format!("SELECT {relation}"))
            .await?
            .first()
            .and_then(|r| r.opt(0))
            .is_none()
    {
        return Err(Error::Other(format!(
            "{} not found",
            Dialect::Postgres.qualified(Some(schema), table)
        )));
    }

    let indexes = indexes(conn, &relation).await?;

    // `ON DELETE CASCADE`; nothing for the default, NO ACTION.
    let action = |on: &str, code: &str| {
        format!(
            "CASE con.{code} WHEN 'r' THEN 'ON {on} RESTRICT' WHEN 'c' THEN 'ON {on} CASCADE'
                 WHEN 'n' THEN 'ON {on} SET NULL' WHEN 'd' THEN 'ON {on} SET DEFAULT' END"
        )
    };
    let (on_delete, on_update) = (
        action("DELETE", "confdeltype"),
        action("UPDATE", "confupdtype"),
    );
    let foreign_keys = query_text(
        conn,
        &format!(
            "SELECT con.conname,
                    array_to_string(ARRAY(
                        SELECT a.attname FROM unnest(con.conkey) WITH ORDINALITY k(n, o)
                        JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.n
                        ORDER BY k.o), chr(31)),
                    fn.nspname, fc.relname,
                    array_to_string(ARRAY(
                        SELECT a.attname FROM unnest(con.confkey) WITH ORDINALITY k(n, o)
                        JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.n
                        ORDER BY k.o), chr(31)),
                    NULLIF(concat_ws(' ',
                        CASE WHEN con.confmatchtype = 'f' THEN 'MATCH FULL' END,
                        {on_delete}, {on_update},
                        CASE WHEN con.condeferrable THEN 'DEFERRABLE'
                             || CASE WHEN con.condeferred THEN ' INITIALLY DEFERRED' ELSE '' END
                        END), '')
             FROM pg_constraint con
             JOIN pg_class fc ON fc.oid = con.confrelid
             JOIN pg_namespace fn ON fn.oid = fc.relnamespace
             WHERE con.contype = 'f' AND con.conrelid = {relation}
             ORDER BY con.conname"
        ),
    )
    .await?
    .into_iter()
    .map(|r| ForeignKeyMeta {
        name: r.text(0),
        columns: list(r.opt(1)),
        ref_schema: r.text(2),
        ref_table: r.text(3),
        ref_columns: list(r.opt(4)),
        detail: r.opt(5),
    })
    .collect();

    let constraints = constraints(conn, &relation).await?;
    let referenced_by = referencing_keys(conn, schema, table).await?;

    Ok(TableDetails {
        columns,
        indexes,
        foreign_keys,
        constraints,
        referenced_by,
    })
}

/// A key part of an index as `pg_get_indexdef` prints it, with a column's name unquoted
/// (`"Customer Id"` gives `Customer Id`); an expression stays as it is.
fn index_column(part: String) -> String {
    let inner = part
        .strip_prefix('"')
        .and_then(|p| p.strip_suffix('"'))
        .filter(|i| !i.replace("\"\"", "").contains('"'));
    match inner {
        Some(name) => name.replace("\"\"", "\""),
        None => part,
    }
}

async fn indexes(conn: &dyn Connection, relation: &str) -> Result<Vec<IndexMeta>> {
    let cockroach = crate::is_cockroach(conn);
    let size = if cockroach {
        "NULL::bigint"
    } else {
        "pg_relation_size(i.indexrelid)"
    };
    // The keys that are not ascending nulls last, and the included columns; CockroachDB's
    // catalog has no `indoption`.
    let (order, include) = if cockroach {
        ("NULL", "NULL")
    } else {
        (
            "array_to_string(ARRAY(
                SELECT pg_get_indexdef(i.indexrelid, k, true)
                       || CASE WHEN i.indoption[k - 1] & 1 = 1 THEN ' DESC' ELSE '' END
                       || CASE WHEN i.indoption[k - 1] & 2 = 2 AND i.indoption[k - 1] & 1 = 0
                               THEN ' NULLS FIRST'
                               WHEN i.indoption[k - 1] & 2 = 0 AND i.indoption[k - 1] & 1 = 1
                               THEN ' NULLS LAST' ELSE '' END
                FROM generate_series(1, i.indnkeyatts) k
                WHERE i.indoption[k - 1] & 3 <> 0
                ORDER BY k), chr(31))",
            "CASE WHEN i.indnatts > i.indnkeyatts THEN 'INCLUDE (' || array_to_string(ARRAY(
                SELECT pg_get_indexdef(i.indexrelid, k, true)
                FROM generate_series(i.indnkeyatts + 1, i.indnatts::int) k ORDER BY k), ', ') || ')' END",
        )
    };
    Ok(query_text(
        conn,
        &format!(
            "SELECT ic.relname, i.indisunique, i.indisprimary,
                    array_to_string(ARRAY(
                        SELECT pg_get_indexdef(i.indexrelid, k, true)
                        FROM generate_series(1, i.indnkeyatts) k ORDER BY k), chr(31)),
                    {size}, i.indpred IS NOT NULL, {order}, {include},
                    pg_get_expr(i.indpred, i.indrelid, true)
             FROM pg_index i JOIN pg_class ic ON ic.oid = i.indexrelid
             WHERE i.indrelid = {relation}
             ORDER BY i.indisprimary DESC, ic.relname"
        ),
    )
    .await?
    .into_iter()
    .map(|r| IndexMeta {
        name: r.text(0),
        unique: r.flag(1),
        primary: r.flag(2),
        partial: r.flag(5),
        columns: list(r.opt(3)).into_iter().map(index_column).collect(),
        size: r.number(4),
        detail: index_detail(&r),
    })
    .collect())
}

/// The sort orders, `INCLUDE` list and predicate of an index row, written as one line.
fn index_detail(r: &TextRow) -> Option<String> {
    let order = list(r.opt(6)).join(", ");
    let predicate = r.opt(8).map(|p| format!("WHERE {p}"));
    let detail = [Some(order), r.opt(7), predicate]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!detail.is_empty()).then_some(detail)
}

/// Primary key, unique, check and exclusion constraints.
async fn constraints(conn: &dyn Connection, relation: &str) -> Result<Vec<ConstraintMeta>> {
    let constraints = query_text(
        conn,
        &format!(
            "SELECT con.conname, con.contype::text,
                    array_to_string(ARRAY(
                        SELECT a.attname FROM unnest(con.conkey) WITH ORDINALITY k(n, o)
                        JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.n
                        ORDER BY k.o), chr(31)),
                    pg_get_constraintdef(con.oid, true)
             FROM pg_constraint con
             WHERE con.conrelid = {relation} AND con.contype IN ('p', 'u', 'c', 'x')
             ORDER BY con.contype = 'p' DESC, con.conname"
        ),
    )
    .await?
    .into_iter()
    .map(|r| ConstraintMeta {
        name: r.text(0),
        kind: match r.text(1).as_str() {
            "p" => ConstraintKind::PrimaryKey,
            "u" => ConstraintKind::Unique,
            "x" => ConstraintKind::Exclusion,
            _ => ConstraintKind::Check,
        },
        columns: list(r.opt(2)),
        definition: r.text(3),
    })
    .collect();
    Ok(constraints)
}

/// Foreign keys of other tables that point at `schema.table`.
pub async fn referencing_keys(
    conn: &dyn Connection,
    schema: &str,
    table: &str,
) -> Result<Vec<ReferencingKey>> {
    let columns = |key: &str, rel: &str| {
        format!(
            "array_to_string(ARRAY(
                SELECT a.attname FROM unnest(con.{key}) WITH ORDINALITY k(n, o)
                JOIN pg_attribute a ON a.attrelid = con.{rel} AND a.attnum = k.n
                ORDER BY k.o), chr(31))"
        )
    };
    Ok(query_text(
        conn,
        &format!(
            "SELECT n.nspname, c.relname, con.conname, {}, {}
             FROM pg_constraint con
             JOIN pg_class c ON c.oid = con.conrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE con.contype = 'f' AND con.confrelid =
                 (SELECT c.oid FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                  WHERE n.nspname = {} AND c.relname = {})
             ORDER BY n.nspname, c.relname, con.conname",
            columns("conkey", "conrelid"),
            columns("confkey", "confrelid"),
            literal(schema),
            literal(table)
        ),
    )
    .await?
    .into_iter()
    .map(|r| ReferencingKey {
        schema: r.text(0),
        table: r.text(1),
        name: r.text(2),
        columns: list(r.opt(3)),
        ref_columns: list(r.opt(4)),
    })
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_columns_are_unquoted() {
        assert_eq!(index_column(r#""Customer Id""#.to_owned()), "Customer Id");
        assert_eq!(index_column(r#""user""#.to_owned()), "user");
        assert_eq!(index_column(r#""a""b""#.to_owned()), r#"a"b"#);
        assert_eq!(index_column("lower(email)".to_owned()), "lower(email)");
        assert_eq!(index_column(r#""a" || "b""#.to_owned()), r#""a" || "b""#);
    }

    #[test]
    fn escapes_literals() {
        assert_eq!(literal(r"it's a \ test"), r"E'it''s a \\ test'");
        assert_eq!(list(Some(format!("a{SEP}b"))), ["a", "b"]);
        assert_eq!(list(Some(String::new())), Vec::<String>::new());
    }
}
