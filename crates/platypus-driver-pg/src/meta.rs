//! Navigator metadata from `pg_catalog`.

use platypus_core::{
    CATALOG_LIMIT, CatalogColumn, ColumnMeta, Connection, ForeignKeyMeta, IndexMeta, ObjectKind,
    ObjectMeta, ReferencingKey, Result, SchemaMeta, TableDetails, TextRowExt, query_catalog,
    query_text,
};

/// Separates list items aggregated into one text cell.
const SEP: char = '\u{1f}';

/// A string literal that is valid whatever `standard_conforming_strings` says.
fn literal(s: &str) -> String {
    format!("E'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
}

fn list(cell: Option<String>) -> Vec<String> {
    cell.filter(|s| !s.is_empty())
        .map(|s| s.split(SEP).map(str::to_owned).collect())
        .unwrap_or_default()
}

pub async fn schemas(conn: &dyn Connection) -> Result<Vec<SchemaMeta>> {
    let rows = query_text(
        conn,
        "SELECT nspname,
                nspname IN ('pg_catalog', 'information_schema'),
                nspname = current_schema()
         FROM pg_namespace
         WHERE nspname NOT LIKE 'pg\\_toast%' AND nspname NOT LIKE 'pg\\_temp\\_%'
         ORDER BY nspname",
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

pub async fn objects(conn: &dyn Connection, schema: &str) -> Result<Vec<ObjectMeta>> {
    let schema = literal(schema);
    let rows = query_text(
        conn,
        &format!(
            "SELECT c.relname, c.relkind::text, obj_description(c.oid, 'pg_class')
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = {schema} AND c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')
             UNION ALL
             SELECT p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')',
                    CASE p.prokind WHEN 'p' THEN 'P' ELSE 'F' END,
                    obj_description(p.oid, 'pg_proc')
             FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
             WHERE n.nspname = {schema} AND p.prokind IN ('f', 'p')
             ORDER BY 1"
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| ObjectMeta {
            name: r.text(0),
            kind: match r.text(1).as_str() {
                "v" => ObjectKind::View,
                "m" => ObjectKind::MaterializedView,
                "f" => ObjectKind::ForeignTable,
                "S" => ObjectKind::Sequence,
                "F" => ObjectKind::Function,
                "P" => ObjectKind::Procedure,
                _ => ObjectKind::Table,
            },
            comment: r.opt(2),
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

    let columns = query_text(
        conn,
        &format!(
            "SELECT a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull,
                    pg_get_expr(d.adbin, d.adrelid),
                    coalesce(a.attnum = ANY (i.indkey), false),
                    col_description(a.attrelid, a.attnum),
                    (SELECT string_agg(e.enumlabel, E'\\n' ORDER BY e.enumsortorder)
                     FROM pg_enum e WHERE e.enumtypid = a.atttypid)
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
    })
    .collect();

    let indexes = query_text(
        conn,
        &format!(
            "SELECT ic.relname, i.indisunique, i.indisprimary,
                    array_to_string(ARRAY(
                        SELECT pg_get_indexdef(i.indexrelid, k, true)
                        FROM generate_series(1, i.indnkeyatts) k ORDER BY k), chr(31))
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
        columns: list(r.opt(3)),
    })
    .collect();

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
                        ORDER BY k.o), chr(31))
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
    })
    .collect();

    Ok(TableDetails {
        columns,
        indexes,
        foreign_keys,
    })
}

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
    fn escapes_literals() {
        assert_eq!(literal(r"it's a \ test"), r"E'it''s a \\ test'");
        assert_eq!(list(Some(format!("a{SEP}b"))), ["a", "b"]);
        assert_eq!(list(Some(String::new())), Vec::<String>::new());
    }
}
