//! `CREATE` statements and properties of objects. PostgreSQL prints views, routines,
//! triggers, indexes and constraints itself (`pg_get_*def`); tables, sequences and types
//! are rebuilt from the catalog.

use std::borrow::Cow;
use std::fmt::Write as _;

use platypus_core::{
    Connection, Error, ObjectKind, ObjectRef, Property, Result, TextRow, TextRowExt, query_text,
    unsupported,
};
use platypus_sql::Dialect;

use crate::meta::{list, literal};

fn ident(name: &str) -> Cow<'_, str> {
    Dialect::Postgres.quote_ident(name)
}

fn qualified(schema: &str, name: &str) -> String {
    Dialect::Postgres.qualified(Some(schema), name)
}

/// The oid of a relation, as a subquery.
fn relation(schema: &str, name: &str) -> String {
    format!(
        "(SELECT c.oid FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
          WHERE n.nspname = {} AND c.relname = {})",
        literal(schema),
        literal(name)
    )
}

/// A routine named like the navigator lists it, `name(identity arguments)`, as an oid
/// subquery. The arguments carry their names and modes (`a integer, OUT b text`), which a
/// cast to `regprocedure` would reject, so the text is matched as `objects` builds it.
fn routine(schema: &str, name: &str) -> String {
    format!(
        "(SELECT p.oid FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
          WHERE n.nspname = {} AND p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')' = {})",
        literal(schema),
        literal(name)
    )
}

async fn row(conn: &dyn Connection, sql: &str, what: &ObjectRef<'_>) -> Result<TextRow> {
    query_text(conn, sql)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| Error::Other(format!("{} not found", qualified(what.schema, what.name))))
}

async fn value(conn: &dyn Connection, sql: &str, what: &ObjectRef<'_>) -> Result<String> {
    Ok(row(conn, sql, what).await?.text(0))
}

/// Ends a statement with exactly one `;`.
fn statement(sql: &str) -> String {
    let sql = sql.trim_end();
    if sql.ends_with(';') {
        sql.to_owned()
    } else {
        format!("{sql};")
    }
}

pub async fn object_ddl(conn: &dyn Connection, o: &ObjectRef<'_>) -> Result<String> {
    let name = qualified(o.schema, o.name);
    match o.kind {
        ObjectKind::Table | ObjectKind::ForeignTable => table(conn, o).await,
        ObjectKind::View | ObjectKind::MaterializedView => {
            let rel = relation(o.schema, o.name);
            let body = value(conn, &format!("SELECT pg_get_viewdef({rel}, true)"), o).await?;
            let create = if o.kind == ObjectKind::View {
                "CREATE OR REPLACE VIEW"
            } else {
                "CREATE MATERIALIZED VIEW"
            };
            Ok(statement(&format!(
                "{create} {name} AS\n{}",
                body.trim_start()
            )))
        }
        ObjectKind::Sequence => sequence(conn, o).await,
        ObjectKind::Function | ObjectKind::Procedure => {
            let sql = format!(
                "SELECT pg_get_functiondef(p.oid) FROM pg_proc p WHERE p.oid = {}",
                routine(o.schema, o.name)
            );
            Ok(statement(&value(conn, &sql, o).await?))
        }
        ObjectKind::Trigger => {
            let table = o
                .parent
                .ok_or_else(|| Error::Other("a trigger needs its table".to_owned()))?;
            let sql = format!(
                "SELECT pg_get_triggerdef(t.oid, true) FROM pg_trigger t
                 WHERE t.tgname = {} AND t.tgrelid = {}",
                literal(o.name),
                relation(o.schema, table)
            );
            Ok(statement(&value(conn, &sql, o).await?))
        }
        ObjectKind::Type | ObjectKind::Domain => type_ddl(conn, o).await,
        ObjectKind::Extension => {
            let sql = format!(
                "SELECT e.extversion FROM pg_extension e WHERE e.extname = {}",
                literal(o.name)
            );
            let version = value(conn, &sql, o).await?;
            Ok(format!(
                "CREATE EXTENSION IF NOT EXISTS {} WITH SCHEMA {} VERSION {};",
                ident(o.name),
                ident(o.schema),
                Dialect::Postgres.quote_literal(&version)
            ))
        }
        ObjectKind::Event | ObjectKind::Dictionary | ObjectKind::Synonym => Err(unsupported(
            "events, dictionaries and synonyms in PostgreSQL",
        )),
    }
}

async fn table(conn: &dyn Connection, o: &ObjectRef<'_>) -> Result<String> {
    let rel = relation(o.schema, o.name);
    let name = qualified(o.schema, o.name);
    let info = row(
        conn,
        &format!(
            "SELECT c.relkind::text, c.relpersistence::text, pg_get_partkeydef(c.oid),
                    (SELECT i.inhparent::regclass::text FROM pg_inherits i
                     WHERE i.inhrelid = c.oid AND c.relispartition),
                    pg_get_expr(c.relpartbound, c.oid),
                    obj_description(c.oid, 'pg_class'),
                    (SELECT s.srvname FROM pg_foreign_table ft
                     JOIN pg_foreign_server s ON s.oid = ft.ftserver WHERE ft.ftrelid = c.oid)
             FROM pg_class c WHERE c.oid = {rel}"
        ),
        o,
    )
    .await?;

    let columns = query_text(
        conn,
        &format!(
            "SELECT a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull,
                    pg_get_expr(d.adbin, d.adrelid), a.attidentity::text, a.attgenerated::text,
                    col_description(a.attrelid, a.attnum)
             FROM pg_attribute a
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
             WHERE a.attrelid = {rel} AND a.attnum > 0 AND NOT a.attisdropped
             ORDER BY a.attnum"
        ),
    )
    .await?;
    let constraints = query_text(
        conn,
        &format!(
            "SELECT con.conname, pg_get_constraintdef(con.oid, true)
             FROM pg_constraint con
             WHERE con.conrelid = {rel} AND con.contype IN ('p', 'u', 'c', 'x', 'f')
             ORDER BY con.contype = 'p' DESC, con.contype = 'f', con.conname"
        ),
    )
    .await?;
    // Indexes that back a constraint are created by it.
    let indexes = query_text(
        conn,
        &format!(
            "SELECT pg_get_indexdef(i.indexrelid)
             FROM pg_index i JOIN pg_class ic ON ic.oid = i.indexrelid
             WHERE i.indrelid = {rel} AND NOT EXISTS (
                 SELECT 1 FROM pg_constraint con
                 WHERE con.conindid = i.indexrelid AND con.conrelid = i.indrelid)
             ORDER BY ic.relname"
        ),
    )
    .await?;

    let indexes: Vec<String> = indexes.iter().map(|i| format!("{};", i.text(0))).collect();
    let sections = [
        create_table(&name, &info, &columns, &constraints),
        indexes.join(
            "
",
        ),
        comments(&name, &info, &columns),
    ];
    Ok(sections
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(
            "

",
        ))
}

/// The `CREATE TABLE` statement from the rows of the table, columns and constraints queries.
fn create_table(
    name: &str,
    info: &TextRow,
    columns: &[TextRow],
    constraints: &[TextRow],
) -> String {
    if let Some(parent) = info.opt(3) {
        // Partitions inherit their columns and constraints.
        return format!(
            "CREATE TABLE {name} PARTITION OF {parent}
    {};",
            info.text(4)
        );
    }
    let mut lines: Vec<String> = columns.iter().map(column_line).collect();
    lines.extend(
        constraints
            .iter()
            .map(|c| format!("CONSTRAINT {} {}", ident(&c.text(0)), c.text(1))),
    );
    let create = match (info.text(0).as_str(), info.text(1).as_str()) {
        ("f", _) => "CREATE FOREIGN TABLE",
        (_, "u") => "CREATE UNLOGGED TABLE",
        _ => "CREATE TABLE",
    };
    let mut sql = format!(
        "{create} {name} (
    {}
)",
        lines.join(
            ",
    "
        )
    );
    if let Some(key) = info.opt(2) {
        let _ = write!(
            sql,
            "
PARTITION BY {key}"
        );
    }
    if let Some(server) = info.opt(6) {
        let _ = write!(
            sql,
            "
SERVER {}",
            ident(&server)
        );
    }
    sql.push(';');
    sql
}

/// `COMMENT ON` statements for the table and its columns.
fn comments(name: &str, info: &TextRow, columns: &[TextRow]) -> String {
    let comment = |target: String, text: String| {
        format!(
            "COMMENT ON {target} IS {};",
            Dialect::Postgres.quote_literal(&text)
        )
    };
    info.opt(5)
        .map(|text| comment(format!("TABLE {name}"), text))
        .into_iter()
        .chain(columns.iter().filter_map(|c| {
            c.opt(6)
                .map(|text| comment(format!("COLUMN {name}.{}", ident(&c.text(0))), text))
        }))
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

/// `name type [GENERATED ...] [DEFAULT ...] [NOT NULL]` from a row of the columns query.
fn column_line(c: &TextRow) -> String {
    let mut line = format!("{} {}", ident(&c.text(0)), c.text(1));
    let default = c.opt(3);
    match (c.text(4).as_str(), c.text(5).as_str(), default) {
        ("a", _, _) => line.push_str(" GENERATED ALWAYS AS IDENTITY"),
        ("d", _, _) => line.push_str(" GENERATED BY DEFAULT AS IDENTITY"),
        (_, "s", Some(expr)) => {
            let _ = write!(line, " GENERATED ALWAYS AS ({expr}) STORED");
        }
        (_, _, Some(expr)) => {
            let _ = write!(line, " DEFAULT {expr}");
        }
        _ => {}
    }
    if c.flag(2) {
        line.push_str(" NOT NULL");
    }
    line
}

async fn sequence(conn: &dyn Connection, o: &ObjectRef<'_>) -> Result<String> {
    let s = row(
        conn,
        &format!(
            "SELECT format_type(s.seqtypid, NULL), s.seqincrement, s.seqmin, s.seqmax,
                    s.seqstart, s.seqcache, s.seqcycle
             FROM pg_sequence s WHERE s.seqrelid = {}",
            relation(o.schema, o.name)
        ),
        o,
    )
    .await?;
    Ok(format!(
        "CREATE SEQUENCE {}\n    AS {}\n    INCREMENT BY {}\n    MINVALUE {}\n    MAXVALUE {}\n    START WITH {}\n    CACHE {}\n    {};",
        qualified(o.schema, o.name),
        s.text(0),
        s.text(1),
        s.text(2),
        s.text(3),
        s.text(4),
        s.text(5),
        if s.flag(6) { "CYCLE" } else { "NO CYCLE" }
    ))
}

async fn type_ddl(conn: &dyn Connection, o: &ObjectRef<'_>) -> Result<String> {
    let t = row(
        conn,
        &format!(
            "SELECT t.typtype::text,
                    (SELECT string_agg(quote_literal(e.enumlabel), ', ' ORDER BY e.enumsortorder)
                     FROM pg_enum e WHERE e.enumtypid = t.oid),
                    format_type(t.typbasetype, t.typtypmod), t.typnotnull, t.typdefault,
                    (SELECT string_agg('CONSTRAINT ' || quote_ident(c.conname) || ' '
                                       || pg_get_constraintdef(c.oid, true), chr(31) ORDER BY c.conname)
                     FROM pg_constraint c WHERE c.contypid = t.oid),
                    (SELECT string_agg(quote_ident(a.attname) || ' '
                                       || format_type(a.atttypid, a.atttypmod), chr(31) ORDER BY a.attnum)
                     FROM pg_attribute a
                     WHERE a.attrelid = t.typrelid AND a.attnum > 0 AND NOT a.attisdropped),
                    (SELECT format_type(r.rngsubtype, NULL) FROM pg_range r WHERE r.rngtypid = t.oid)
             FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
             WHERE n.nspname = {} AND t.typname = {}",
            literal(o.schema),
            literal(o.name)
        ),
        o,
    )
    .await?;
    let name = qualified(o.schema, o.name);
    Ok(match t.text(0).as_str() {
        "e" => format!("CREATE TYPE {name} AS ENUM ({});", t.text(1)),
        "d" => {
            let mut sql = format!("CREATE DOMAIN {name} AS {}", t.text(2));
            if let Some(default) = t.opt(4) {
                let _ = write!(sql, " DEFAULT {default}");
            }
            if t.flag(3) {
                sql.push_str(" NOT NULL");
            }
            for constraint in list(t.opt(5)) {
                let _ = write!(sql, "\n    {constraint}");
            }
            format!("{sql};")
        }
        "c" => format!(
            "CREATE TYPE {name} AS (\n    {}\n);",
            list(t.opt(6)).join(",\n    ")
        ),
        "r" => format!("CREATE TYPE {name} AS RANGE (SUBTYPE = {});", t.text(7)),
        other => return Err(unsupported(&format!("DDL of type kind `{other}`"))),
    })
}

pub async fn object_properties(conn: &dyn Connection, o: &ObjectRef<'_>) -> Result<Vec<Property>> {
    if o.kind.is_relation() || o.kind == ObjectKind::Sequence {
        let sizes = if crate::is_cockroach(conn) {
            "NULL, NULL, NULL"
        } else {
            "pg_size_pretty(pg_total_relation_size(c.oid)),
                        pg_size_pretty(pg_relation_size(c.oid)),
                        pg_size_pretty(pg_indexes_size(c.oid))"
        };
        let r = row(
            conn,
            &format!(
                "SELECT pg_get_userbyid(c.relowner), c.reltuples::bigint, {sizes},
                        coalesce(ts.spcname, 'default')
                 FROM pg_class c LEFT JOIN pg_tablespace ts ON ts.oid = c.reltablespace
                 WHERE c.oid = {}",
                relation(o.schema, o.name)
            ),
            o,
        )
        .await?;
        let mut props = vec![Property::new("Owner", r.text(0))];
        if matches!(o.kind, ObjectKind::Table | ObjectKind::MaterializedView) {
            // -1 until the table is first analyzed or vacuumed.
            let rows = r.text(1);
            let estimate = if rows.starts_with('-') {
                "unknown (not analyzed yet)".to_owned()
            } else {
                rows
            };
            props.extend([
                Property::new("Estimated rows", estimate),
                Property::new("Total size", r.text(2)),
                Property::new("Table size", r.text(3)),
                Property::new("Indexes size", r.text(4)),
                Property::new("Tablespace", r.text(5)),
            ]);
        }
        return Ok(props);
    }
    if matches!(o.kind, ObjectKind::Function | ObjectKind::Procedure) {
        let r = row(
            conn,
            &format!(
                "SELECT pg_get_userbyid(p.proowner), l.lanname, pg_get_function_result(p.oid),
                        CASE p.provolatile WHEN 'i' THEN 'immutable' WHEN 's' THEN 'stable'
                                           ELSE 'volatile' END,
                        p.prosecdef
                 FROM pg_proc p JOIN pg_language l ON l.oid = p.prolang
                 WHERE p.oid = {}",
                routine(o.schema, o.name)
            ),
            o,
        )
        .await?;
        let mut props = vec![
            Property::new("Owner", r.text(0)),
            Property::new("Language", r.text(1)),
        ];
        if let Some(returns) = r.opt(2) {
            props.push(Property::new("Returns", returns));
        }
        props.extend([
            Property::new("Volatility", r.text(3)),
            Property::new("Security", if r.flag(4) { "definer" } else { "invoker" }),
        ]);
        return Ok(props);
    }
    Ok(Vec::new())
}

pub async fn databases(conn: &dyn Connection) -> Result<Vec<String>> {
    Ok(query_text(
        conn,
        "SELECT datname FROM pg_database
         WHERE datallowconn AND NOT datistemplate ORDER BY datname",
    )
    .await?
    .iter()
    .map(|r| r.text(0))
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_routines_by_signature() {
        let add = routine("demo", "Add(a integer, b integer)");
        assert!(add.contains("n.nspname = E'demo'"), "{add}");
        assert!(add.contains(" = E'Add(a integer, b integer)')"), "{add}");
        assert_eq!(statement("SELECT 1 ;\n"), "SELECT 1 ;");
        assert_eq!(statement("x"), "x;");
    }

    #[test]
    fn prints_columns() {
        let row =
            |v: [Option<&str>; 7]| -> TextRow { v.iter().map(|c| c.map(str::to_owned)).collect() };
        assert_eq!(
            column_line(&row([
                Some("id"),
                Some("bigint"),
                Some("t"),
                None,
                Some("a"),
                Some(""),
                None
            ])),
            "id bigint GENERATED ALWAYS AS IDENTITY NOT NULL"
        );
        assert_eq!(
            column_line(&row([
                Some("Total"),
                Some("numeric(12,2)"),
                Some("f"),
                Some("0"),
                Some(""),
                Some(""),
                None
            ])),
            "\"Total\" numeric(12,2) DEFAULT 0"
        );
        assert_eq!(
            column_line(&row([
                Some("t2"),
                Some("integer"),
                Some("f"),
                Some("(a * 2)"),
                Some(""),
                Some("s"),
                None
            ])),
            "t2 integer GENERATED ALWAYS AS ((a * 2)) STORED"
        );
    }
}
