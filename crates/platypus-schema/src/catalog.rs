//! What [`crate::load`] reads from the catalogs beyond a table's details, per database: identity
//! and generated columns, collations, what an index is beyond its columns (expressions, prefix
//! lengths, kinds, `INCLUDE`, `WHERE`), partitioning and engine clauses, the user-defined types
//! the columns use. A schema comparison copies them, and the table editor keeps them when it
//! redefines a column.

use std::fmt::Write;

use platypus_core::{Connection, Result, TextRow, TextRowExt, query_text};
use platypus_sql::Dialect;

use crate::{IndexDesign, PartitionDesign, TableDesign, UserType, create};

/// The rows of `sql`, none when the catalog cannot answer it: PostgreSQL's dialect also
/// speaks to CockroachDB and YugabyteDB, which lack some of these catalogs; the design then
/// keeps what the details say.
async fn rows(conn: &dyn Connection, sql: &str) -> Vec<TextRow> {
    query_text(conn, sql).await.unwrap_or_default()
}

/// `expr` in parentheses, unless it is already all in a pair of them.
fn parenthesized(expr: &str) -> String {
    let expr = expr.trim();
    match expr.strip_prefix('(').and_then(|e| e.strip_suffix(')')) {
        Some(inner) if crate::balanced(inner) => expr.to_owned(),
        _ => format!("({expr})"),
    }
}

/// The items of a list (`a, lower(b), c DESC`), split at its top-level commas, quotes kept.
pub(crate) fn split_list(list: &str) -> Vec<String> {
    let mut items = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None, 0);
    for (i, c) in list.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"' | '`' | '[') => quote = Some(if c == '[' { ']' } else { c }),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, ',') if depth == 0 => {
                items.push(list[start..i].trim().to_owned());
                start = i + 1;
            }
            _ => {}
        }
    }
    let tail = list[start..].trim();
    if !tail.is_empty() {
        items.push(tail.to_owned());
    }
    items
}

/// The key list of a `CREATE INDEX .. USING method (..)` as `pg_get_indexdef` prints it.
fn index_keys(definition: &str) -> Option<&str> {
    let using = definition.find(" USING ")?;
    let open = using + definition[using..].find('(')?;
    let (mut depth, mut quote) = (0i32, None);
    for (i, c) in definition[open..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => {
                depth -= 1;
                if depth == 0 {
                    return Some(&definition[open + 1..open + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The key parts of `index` worth keeping: none when each is just its column, quoted.
fn kept_parts(dialect: Dialect, index: &IndexDesign, parts: Vec<String>) -> Vec<String> {
    let plain = parts.len() == index.columns.len()
        && parts
            .iter()
            .zip(&index.columns)
            .all(|(p, c)| *p == dialect.quote_ident(c));
    if plain { Vec::new() } else { parts }
}

/// PostgreSQL: identity and generated columns, collations, index kinds, expressions,
/// `INCLUDE` and `WHERE`, partitioning and the user-defined types of the columns.
pub(crate) async fn postgres(
    conn: &dyn Connection,
    design: &mut TableDesign,
    schema: &str,
    table: &str,
) {
    let d = Dialect::Postgres;
    let rel = format!(
        "{}::regclass",
        d.quote_literal(&d.qualified(Some(schema), table))
    );
    postgres_columns(conn, design, &rel).await;
    postgres_indexes(conn, design, &rel).await;
    let partitioning = rows(
        conn,
        &format!(
            "SELECT pg_get_partkeydef({rel}), c.relname, pg_get_expr(c.relpartbound, c.oid)
             FROM (SELECT 1) one
             LEFT JOIN pg_inherits i ON i.inhparent = {rel}
             LEFT JOIN pg_class c ON c.oid = i.inhrelid AND c.relispartition
             ORDER BY c.relname"
        ),
    )
    .await;
    for row in &partitioning {
        if let Some(key) = row.opt(0) {
            design.options = Some(format!("PARTITION BY {key}"));
        }
        if let (Some(name), Some(bound)) = (row.opt(1), row.opt(2)) {
            design.partitions.push(PartitionDesign { name, bound });
        }
    }
    design.types = postgres_types(conn, &rel).await;
}

async fn postgres_columns(conn: &dyn Connection, design: &mut TableDesign, rel: &str) {
    let sql = format!(
        "SELECT a.attname, a.attidentity::text, a.attgenerated::text,
                pg_get_expr(ad.adbin, ad.adrelid), s.seqstart, s.seqincrement,
                CASE WHEN a.attcollation <> 0 AND a.attcollation <> t.typcollation THEN
                     CASE WHEN cn.nspname = 'pg_catalog' THEN quote_ident(co.collname)
                          ELSE quote_ident(cn.nspname) || '.' || quote_ident(co.collname) END
                END
         FROM pg_attribute a
         JOIN pg_type t ON t.oid = a.atttypid
         LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum
         LEFT JOIN pg_collation co ON co.oid = a.attcollation
         LEFT JOIN pg_namespace cn ON cn.oid = co.collnamespace
         LEFT JOIN pg_sequence s ON a.attidentity IN ('a', 'd')
              AND s.seqrelid = pg_get_serial_sequence(a.attrelid::regclass::text, a.attname)::regclass
         WHERE a.attrelid = {rel} AND a.attnum > 0 AND NOT a.attisdropped"
    );
    for row in rows(conn, &sql).await {
        let Some(column) = design.columns.iter_mut().find(|c| c.name == row.text(0)) else {
            continue;
        };
        let kind = match row.text(1).as_str() {
            "a" => Some("ALWAYS"),
            "d" => Some("BY DEFAULT"),
            _ => None,
        };
        if let Some(kind) = kind {
            let mut identity = format!("GENERATED {kind} AS IDENTITY");
            let (start, step) = (row.text(4), row.text(5));
            if (start.as_str(), step.as_str()) != ("1", "1") && !start.is_empty() {
                let _ = write!(identity, " (START WITH {start} INCREMENT BY {step})");
            }
            column.extra = Some(identity);
            column.default = None;
        }
        let storage = match row.text(2).as_str() {
            "s" => Some("STORED"),
            "v" => Some("VIRTUAL"),
            _ => None,
        };
        if let (Some(storage), Some(expr)) = (storage, row.opt(3)) {
            column.generated = Some(format!(
                "GENERATED ALWAYS AS {} {storage}",
                parenthesized(&expr)
            ));
            column.default = None;
        }
        column.collation = row.opt(6);
    }
}

async fn postgres_indexes(conn: &dyn Connection, design: &mut TableDesign, rel: &str) {
    let sql = format!(
        "SELECT ic.relname, am.amname, pg_get_indexdef(i.indexrelid),
                pg_get_expr(i.indpred, i.indrelid),
                array_to_string(ARRAY(
                    SELECT a.attname FROM unnest(i.indkey::int2[]) WITH ORDINALITY k(n, o)
                    JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.n
                    WHERE k.o > i.indnkeyatts ORDER BY k.o), chr(31))
         FROM pg_index i
         JOIN pg_class ic ON ic.oid = i.indexrelid
         JOIN pg_am am ON am.oid = ic.relam
         WHERE i.indrelid = {rel}"
    );
    for row in rows(conn, &sql).await {
        let Some(index) = design.indexes.iter_mut().find(|i| i.name == row.text(0)) else {
            continue;
        };
        let method = row.text(1);
        index.method = (!method.is_empty() && method != "btree").then_some(method);
        let parts = index_keys(&row.text(2)).map(split_list).unwrap_or_default();
        index.parts = kept_parts(Dialect::Postgres, index, parts);
        index.predicate = row.opt(3);
        index.include = row
            .opt(4)
            .map(|l| {
                l.split('\u{1f}')
                    .filter(|c| !c.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
    }
}

/// The enums, domains, composite and range types of other schemas than the system ones that
/// the columns of `rel` use, arrays of them included.
async fn postgres_types(conn: &dyn Connection, rel: &str) -> Vec<UserType> {
    let sql = format!(
        "SELECT DISTINCT n.nspname, t.typname, t.typtype::text,
                (SELECT string_agg(quote_literal(e.enumlabel), ', ' ORDER BY e.enumsortorder)
                 FROM pg_enum e WHERE e.enumtypid = t.oid),
                format_type(t.typbasetype, t.typtypmod), t.typnotnull, t.typdefault,
                (SELECT string_agg('CONSTRAINT ' || quote_ident(c.conname) || ' '
                                   || pg_get_constraintdef(c.oid, true), ' ' ORDER BY c.conname)
                 FROM pg_constraint c WHERE c.contypid = t.oid),
                (SELECT string_agg(quote_ident(f.attname) || ' '
                                   || format_type(f.atttypid, f.atttypmod), ', ' ORDER BY f.attnum)
                 FROM pg_attribute f
                 WHERE f.attrelid = t.typrelid AND f.attnum > 0 AND NOT f.attisdropped),
                (SELECT format_type(r.rngsubtype, NULL) FROM pg_range r WHERE r.rngtypid = t.oid)
         FROM pg_attribute a
         JOIN pg_type at ON at.oid = a.atttypid
         JOIN pg_type t ON t.oid = CASE WHEN at.typelem <> 0 AND at.typlen = -1
                                        THEN at.typelem ELSE at.oid END
         JOIN pg_namespace n ON n.oid = t.typnamespace
         LEFT JOIN pg_class tc ON tc.oid = t.typrelid
         WHERE a.attrelid = {rel} AND a.attnum > 0 AND NOT a.attisdropped
           AND n.nspname NOT IN ('pg_catalog', 'information_schema')
           AND (t.typtype IN ('e', 'd', 'r') OR (t.typtype = 'c' AND tc.relkind = 'c'))
         ORDER BY 1, 2"
    );
    rows(conn, &sql)
        .await
        .iter()
        .filter_map(|r| {
            let (kind, body) = match r.text(2).as_str() {
                "e" => ("TYPE", format!("AS ENUM ({})", r.text(3))),
                "d" => {
                    let mut body = format!("AS {}", r.text(4));
                    if let Some(default) = r.opt(6) {
                        let _ = write!(body, " DEFAULT {default}");
                    }
                    if r.flag(5) {
                        body.push_str(" NOT NULL");
                    }
                    if let Some(checks) = r.opt(7) {
                        body.push(' ');
                        body.push_str(&checks);
                    }
                    ("DOMAIN", body)
                }
                "c" => ("TYPE", format!("AS ({})", r.text(8))),
                "r" => ("TYPE", format!("AS RANGE (SUBTYPE = {})", r.text(9))),
                _ => return None,
            };
            Some(UserType {
                schema: r.text(0),
                name: r.text(1),
                kind: kind.to_owned(),
                body,
            })
        })
        .collect()
}

/// SQL Server: identity and computed columns, collations other than the database's, and
/// clustered, filtered and descending indexes and their `INCLUDE` columns.
pub(crate) async fn sqlserver(
    conn: &dyn Connection,
    design: &mut TableDesign,
    schema: &str,
    table: &str,
) -> Result<()> {
    let d = Dialect::SqlServer;
    let id = format!(
        "OBJECT_ID({})",
        d.quote_literal(&d.qualified(Some(schema), table))
    );
    let sql = format!(
        "SELECT c.name, c.is_identity, CAST(ic.seed_value AS nvarchar(40)),
                CAST(ic.increment_value AS nvarchar(40)), cc.definition, cc.is_persisted,
                c.collation_name, CAST(DATABASEPROPERTYEX(DB_NAME(), 'Collation') AS nvarchar(128))
         FROM sys.columns c
         LEFT JOIN sys.identity_columns ic
           ON ic.object_id = c.object_id AND ic.column_id = c.column_id
         LEFT JOIN sys.computed_columns cc
           ON cc.object_id = c.object_id AND cc.column_id = c.column_id
         WHERE c.object_id = {id}"
    );
    for row in query_text(conn, &sql).await? {
        let Some(column) = design.columns.iter_mut().find(|c| c.name == row.text(0)) else {
            continue;
        };
        if row.flag(1) {
            column.extra = Some(format!("IDENTITY({},{})", row.text(2), row.text(3)));
            column.default = None;
        }
        if let Some(expr) = row.opt(4) {
            let persisted = if row.flag(5) { " PERSISTED" } else { "" };
            column.generated = Some(format!("AS {}{persisted}", parenthesized(&expr)));
            column.default = None;
        } else {
            column.collation = row.opt(6).filter(|c| Some(c) != row.opt(7).as_ref());
        }
    }
    let sql = format!(
        "SELECT i.name, i.type_desc, i.filter_definition, COL_NAME(ic.object_id, ic.column_id),
                ic.is_descending_key, ic.is_included_column
         FROM sys.indexes i
         JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id
         WHERE i.object_id = {id} AND i.is_primary_key = 0 AND i.is_hypothetical = 0
         ORDER BY i.name, ic.is_included_column, ic.key_ordinal, ic.index_column_id"
    );
    let mut keys: Vec<(String, Vec<String>, bool)> = Vec::new();
    for row in query_text(conn, &sql).await? {
        let name = row.text(0);
        let Some(index) = design.indexes.iter_mut().find(|i| i.name == name) else {
            continue;
        };
        let kind = row.text(1);
        index.method = (kind != "NONCLUSTERED").then(|| kind.replace('_', " "));
        index.predicate = row.opt(2);
        let column = row.text(3);
        if row.flag(5) {
            if !index.include.contains(&column) {
                index.include.push(column);
            }
            continue;
        }
        let part = format!(
            "{}{}",
            d.quote_ident(&column),
            if row.flag(4) { " DESC" } else { "" }
        );
        match keys.iter_mut().find(|(n, ..)| *n == name) {
            Some((_, parts, descending)) => {
                parts.push(part);
                *descending |= row.flag(4);
            }
            None => keys.push((name, vec![part], row.flag(4))),
        }
    }
    for (name, parts, descending) in keys {
        if let Some(index) = design.indexes.iter_mut().find(|i| i.name == name) {
            index.parts = if descending { parts } else { Vec::new() };
        }
    }
    Ok(())
}

/// ClickHouse: the engine clause (`ORDER BY`, `PARTITION BY`, `TTL`, `SETTINGS`) and the
/// expression and type of its data skipping indexes.
pub(crate) async fn clickhouse(
    conn: &dyn Connection,
    design: &mut TableDesign,
    schema: &str,
    table: &str,
) -> Result<()> {
    let d = Dialect::ClickHouse;
    let (s, t) = (d.quote_literal(schema), d.quote_literal(table));
    let sql = format!("SELECT engine_full FROM system.tables WHERE database = {s} AND name = {t}");
    design.options = query_text(conn, &sql)
        .await?
        .first()
        .and_then(|r| r.opt(0))
        .filter(|e| !e.trim().is_empty())
        .map(|e| format!("ENGINE = {e}"));
    let sql = format!(
        "SELECT name, expr, type_full, granularity FROM system.data_skipping_indices
         WHERE database = {s} AND table = {t}"
    );
    for row in query_text(conn, &sql).await? {
        if let Some(index) = design.indexes.iter_mut().find(|i| i.name == row.text(0)) {
            index.parts = vec![row.text(1)];
            index.method = Some(format!("TYPE {} GRANULARITY {}", row.text(2), row.text(3)));
        }
    }
    Ok(())
}

/// MySQL and MariaDB: the kind and key parts (prefix lengths, expressions, `DESC`) of the
/// indexes `SHOW CREATE TABLE` lists, and MariaDB's JSON columns without the CHECK
/// (`json_valid`) and collation the type brings by itself.
pub(crate) fn mysql(design: &mut TableDesign, definition: &create::Definition) {
    for parsed in &definition.indexes {
        if let Some(index) = design.indexes.iter_mut().find(|i| i.name == parsed.name) {
            index.method.clone_from(&parsed.method);
            index.parts = kept_parts(Dialect::MySql, index, parsed.parts.clone());
        }
    }
    let json: Vec<String> = design
        .columns
        .iter_mut()
        .filter(|c| c.data_type.eq_ignore_ascii_case("json"))
        .map(|c| {
            c.collation = None;
            c.name.clone()
        })
        .collect();
    design.constraints.retain(|c| {
        let implicit = json.contains(&c.name)
            && c.check
                .replace(['`', ' ', '(', ')'], "")
                .to_ascii_lowercase()
                == format!("json_valid{}", c.name.to_ascii_lowercase());
        !implicit
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_split_at_top_level_commas() {
        assert_eq!(
            split_list("lower(name), \"a,b\" DESC, coalesce(x, 0)"),
            ["lower(name)", "\"a,b\" DESC", "coalesce(x, 0)"]
        );
        assert_eq!(split_list("`body`(50)"), ["`body`(50)"]);
        assert_eq!(split_list(" "), Vec::<String>::new());
    }

    #[test]
    fn index_keys_come_from_the_definition() {
        assert_eq!(
            index_keys("CREATE INDEX t_lower ON s.t USING btree (lower(name)) WHERE (id > 5)"),
            Some("lower(name)")
        );
        assert_eq!(
            index_keys("CREATE INDEX i ON s.t USING btree (a, \"b)\" DESC) INCLUDE (c)"),
            Some("a, \"b)\" DESC")
        );
    }

    #[test]
    fn plain_keys_are_not_kept() {
        let index = IndexDesign {
            columns: vec!["a".into(), "My Col".into()],
            ..IndexDesign::default()
        };
        let plain = vec!["a".to_owned(), "\"My Col\"".to_owned()];
        assert_eq!(
            kept_parts(Dialect::Postgres, &index, plain),
            Vec::<String>::new()
        );
        let desc = vec!["a DESC".to_owned(), "\"My Col\"".to_owned()];
        assert_eq!(kept_parts(Dialect::Postgres, &index, desc.clone()), desc);
    }

    #[test]
    fn expressions_get_one_pair_of_parentheses() {
        assert_eq!(parenthesized("(a * 2)"), "(a * 2)");
        assert_eq!(parenthesized("a * 2"), "(a * 2)");
        assert_eq!(parenthesized("(a) + (b)"), "((a) + (b))");
    }

    #[test]
    fn mariadb_json_columns_lose_their_implicit_check() {
        let mut design = TableDesign {
            columns: vec![crate::ColumnDesign {
                name: "j".into(),
                data_type: "json".into(),
                collation: Some("utf8mb4_bin".into()),
                ..crate::ColumnDesign::default()
            }],
            constraints: vec![
                crate::ConstraintDesign {
                    name: "j".into(),
                    kind: crate::ConstraintType::Check,
                    check: "json_valid(`j`)".into(),
                    ..crate::ConstraintDesign::default()
                },
                crate::ConstraintDesign {
                    name: "other".into(),
                    kind: crate::ConstraintType::Check,
                    check: "json_valid(`j`)".into(),
                    ..crate::ConstraintDesign::default()
                },
            ],
            ..TableDesign::default()
        };
        mysql(&mut design, &create::Definition::default());
        assert_eq!(design.columns[0].collation, None);
        let names: Vec<&str> = design.constraints.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["other"]);
    }
}
