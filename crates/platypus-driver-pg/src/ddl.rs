//! `CREATE` statements and properties of objects. PostgreSQL prints views, routines,
//! triggers, indexes and constraints itself (`pg_get_*def`); tables, sequences and types
//! are rebuilt from the catalog.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;

use platypus_core::{
    Connection, Error, ObjectKind, ObjectRef, Property, Result, TextRow, TextRowExt, query_text,
    unsupported,
};
use platypus_sql::Dialect;

use crate::meta::{COLUMN_TYPE, list, literal};

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

/// The DDL of `objects`: tables and views with a few catalog queries for all of them, the
/// other kinds one at a time.
pub async fn objects_ddl(conn: &dyn Connection, objects: &[ObjectRef<'_>]) -> Vec<Result<String>> {
    let of_kind = |pick: fn(ObjectKind) -> bool| -> Vec<ObjectRef<'_>> {
        objects.iter().filter(|o| pick(o.kind)).copied().collect()
    };
    let mut tables = tables_ddl(conn, &of_kind(is_table)).await.into_iter();
    let mut views = views_ddl(conn, &of_kind(is_view)).await.into_iter();
    let mut out = Vec::with_capacity(objects.len());
    for o in objects {
        let ddl = if is_table(o.kind) {
            tables.next()
        } else if is_view(o.kind) {
            views.next()
        } else {
            Some(object_ddl(conn, o).await)
        };
        out.push(ddl.unwrap_or_else(no_ddl));
    }
    out
}

fn is_table(kind: ObjectKind) -> bool {
    matches!(kind, ObjectKind::Table | ObjectKind::ForeignTable)
}

fn is_view(kind: ObjectKind) -> bool {
    matches!(kind, ObjectKind::View | ObjectKind::MaterializedView)
}

fn no_ddl() -> Result<String> {
    Err(Error::Other("no DDL read".to_owned()))
}

pub async fn object_ddl(conn: &dyn Connection, o: &ObjectRef<'_>) -> Result<String> {
    match o.kind {
        ObjectKind::Table | ObjectKind::ForeignTable => single(tables_ddl(conn, &[*o]).await),
        ObjectKind::View | ObjectKind::MaterializedView => single(views_ddl(conn, &[*o]).await),
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
        ObjectKind::Event | ObjectKind::Dictionary | ObjectKind::Synonym | ObjectKind::Package => {
            Err(unsupported(
                "events, dictionaries, synonyms and packages in PostgreSQL",
            ))
        }
    }
}

/// Whether the TimescaleDB extension is installed in the database.
async fn has_timescale(conn: &dyn Connection) -> Result<bool> {
    let rows = query_text(
        conn,
        "SELECT 1 FROM pg_extension WHERE extname = 'timescaledb'",
    )
    .await?;
    Ok(!rows.is_empty())
}

/// The `TimescaleDB` hypertable `o`'s row estimate and sizes, in the columns of the
/// properties query; `None` for any other table.
async fn hypertable_sizes(conn: &dyn Connection, o: &ObjectRef<'_>) -> Result<Option<TextRow>> {
    if !has_timescale(conn).await? {
        return Ok(None);
    }
    let rows = query_text(
        conn,
        &format!(
            "SELECT NULL, approximate_row_count(h.oid),
                    pg_size_pretty(s.total_bytes), pg_size_pretty(s.table_bytes),
                    pg_size_pretty(s.index_bytes),
                    (SELECT coalesce(ts.spcname, 'default') FROM pg_class c
                     LEFT JOIN pg_tablespace ts ON ts.oid = c.reltablespace WHERE c.oid = h.oid)
             FROM (SELECT {} AS oid) h, hypertable_detailed_size(h.oid) s
             WHERE EXISTS (SELECT 1 FROM timescaledb_information.hypertables i
                           WHERE i.hypertable_schema = {} AND i.hypertable_name = {})",
            relation(o.schema, o.name),
            literal(o.schema),
            literal(o.name)
        ),
    )
    .await?;
    Ok(rows.into_iter().next())
}

/// The statements that make table `name` the `TimescaleDB` hypertable it is: its dimensions
/// and compression. Empty for any other table. The caller checks that `TimescaleDB` is installed.
async fn hypertable(conn: &dyn Connection, o: &ObjectRef<'_>, name: &str) -> Result<String> {
    let (schema, table) = (literal(o.schema), literal(o.name));
    let dimensions = query_text(
        conn,
        &format!(
            "SELECT column_name, dimension_type, time_interval::text, integer_interval::text,
                    num_partitions::text
             FROM timescaledb_information.dimensions
             WHERE hypertable_schema = {schema} AND hypertable_name = {table}
             ORDER BY dimension_number"
        ),
    )
    .await?;
    let target = Dialect::Postgres.quote_literal(name);
    let mut out = Vec::new();
    for (i, d) in dimensions.iter().enumerate() {
        let column = Dialect::Postgres.quote_literal(&d.text(0));
        let interval = match (d.opt(2), d.opt(3)) {
            (Some(t), _) => format!("INTERVAL {}", Dialect::Postgres.quote_literal(&t)),
            (None, Some(n)) => n,
            (None, None) => String::new(),
        };
        out.push(match (i, d.opt(4)) {
            (0, _) => format!(
                "SELECT create_hypertable({target}, {column}, chunk_time_interval => {interval});"
            ),
            (_, Some(partitions)) => format!(
                "SELECT add_dimension({target}, {column}, number_partitions => {partitions});"
            ),
            _ => format!(
                "SELECT add_dimension({target}, {column}, chunk_time_interval => {interval});"
            ),
        });
    }
    if out.is_empty() {
        return Ok(String::new());
    }
    let compression = query_text(
        conn,
        &format!(
            "SELECT h.compression_enabled,
                    (SELECT string_agg(quote_ident(s.attname), ', ' ORDER BY s.segmentby_column_index)
                     FROM timescaledb_information.compression_settings s
                     WHERE s.hypertable_schema = h.hypertable_schema
                       AND s.hypertable_name = h.hypertable_name
                       AND s.segmentby_column_index IS NOT NULL),
                    (SELECT string_agg(quote_ident(s.attname)
                                       || CASE WHEN s.orderby_asc THEN '' ELSE ' DESC' END,
                                       ', ' ORDER BY s.orderby_column_index)
                     FROM timescaledb_information.compression_settings s
                     WHERE s.hypertable_schema = h.hypertable_schema
                       AND s.hypertable_name = h.hypertable_name
                       AND s.orderby_column_index IS NOT NULL)
             FROM timescaledb_information.hypertables h
             WHERE h.hypertable_schema = {schema} AND h.hypertable_name = {table}"
        ),
    )
    .await?;
    if let Some(c) = compression.first().filter(|c| c.flag(0)) {
        let mut options = vec!["timescaledb.compress".to_owned()];
        if let Some(by) = c.opt(1) {
            options.push(format!(
                "timescaledb.compress_segmentby = {}",
                Dialect::Postgres.quote_literal(&by)
            ));
        }
        if let Some(by) = c.opt(2) {
            options.push(format!(
                "timescaledb.compress_orderby = {}",
                Dialect::Postgres.quote_literal(&by)
            ));
        }
        out.push(format!("ALTER TABLE {name} SET ({});", options.join(", ")));
    }
    Ok(out.join("\n"))
}

/// YugabyteDB's sharding of table `rel`: its primary key's columns with their `HASH` or
/// `ASC`/`DESC` (`pg_get_constraintdef` leaves them out, and a key without them is sharded
/// by hash), and how its tablets are split.
async fn yugabyte_sharding(
    conn: &dyn Connection,
    rel: &str,
) -> Result<(Option<String>, Option<String>)> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT (SELECT pg_get_indexdef(con.conindid) FROM pg_constraint con
                     WHERE con.conrelid = {rel} AND con.contype = 'p'),
                    yb_get_range_split_clause({rel}),
                    (SELECT p.num_tablets::text FROM yb_table_properties({rel}) p
                     WHERE p.num_hash_key_columns > 0)"
        ),
    )
    .await?;
    let Some(r) = rows.first() else {
        return Ok((None, None));
    };
    // `CREATE UNIQUE INDEX r_pkey ON s.r USING lsm (id ASC)`.
    let key = r.opt(0).and_then(|def| {
        let columns = def.rsplit_once(" USING ")?.1;
        let columns = &columns[columns.find('(')?..];
        Some(format!("PRIMARY KEY {columns}"))
    });
    let split = r
        .opt(1)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| r.opt(2).map(|n| format!("SPLIT INTO {n} TABLETS")));
    Ok((key, split))
}

/// The one result of a reader called with one object.
fn single(mut results: Vec<Result<String>>) -> Result<String> {
    results.pop().unwrap_or_else(no_ddl)
}

/// `(schema, name) IN (…)` of `objects`, for `pg_namespace n` and `pg_class c`.
fn named(objects: &[ObjectRef<'_>]) -> String {
    let pairs: Vec<String> = objects
        .iter()
        .map(|o| format!("({}, {})", literal(o.schema), literal(o.name)))
        .collect();
    format!(
        "(n.nspname::text, c.relname::text) IN ({})",
        pairs.join(", ")
    )
}

/// The oids of the relations `objects`, by schema and name.
async fn relation_oids(
    conn: &dyn Connection,
    objects: &[ObjectRef<'_>],
) -> Result<HashMap<(String, String), String>> {
    let rows = query_text(
        conn,
        &format!(
            "SELECT c.oid::text, n.nspname, c.relname FROM pg_class c
             JOIN pg_namespace n ON n.oid = c.relnamespace WHERE {}",
            named(objects)
        ),
    )
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ((r.text(1), r.text(2)), r.text(0)))
        .collect())
}

/// The rows of `sql`, whose first column is a relation's oid, by that oid and without it.
async fn by_relation(conn: &dyn Connection, sql: &str) -> Result<HashMap<String, Vec<TextRow>>> {
    let mut out: HashMap<String, Vec<TextRow>> = HashMap::new();
    for mut row in query_text(conn, sql).await? {
        let oid = row.remove(0).unwrap_or_default();
        out.entry(oid).or_default().push(row);
    }
    Ok(out)
}

/// The same failure for every object of a reader whose query failed.
fn failed(objects: &[ObjectRef<'_>], e: &Error) -> Vec<Result<String>> {
    objects
        .iter()
        .map(|_| Err(Error::Other(e.to_string())))
        .collect()
}

fn not_found(o: &ObjectRef<'_>) -> Error {
    Error::Other(format!("{} not found", qualified(o.schema, o.name)))
}

/// The DDL of views and materialized views, read with one query.
async fn views_ddl(conn: &dyn Connection, objects: &[ObjectRef<'_>]) -> Vec<Result<String>> {
    if objects.is_empty() {
        return Vec::new();
    }
    let sql = format!(
        "SELECT n.nspname || chr(1) || c.relname, pg_get_viewdef(c.oid, true) FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace WHERE {}",
        named(objects)
    );
    let bodies = match by_relation(conn, &sql).await {
        Ok(bodies) => bodies,
        Err(e) => return failed(objects, &e),
    };
    objects
        .iter()
        .map(|o| {
            let body = bodies
                .get(&format!("{}\u{1}{}", o.schema, o.name))
                .and_then(|rows| rows.first())
                .ok_or_else(|| not_found(o))?
                .text(0);
            let create = if o.kind == ObjectKind::View {
                "CREATE OR REPLACE VIEW"
            } else {
                "CREATE MATERIALIZED VIEW"
            };
            Ok(statement(&format!(
                "{create} {} AS\n{}",
                qualified(o.schema, o.name),
                body.trim_start()
            )))
        })
        .collect()
}

/// What the catalog queries read about tables, by oid.
struct TableRows {
    info: HashMap<String, Vec<TextRow>>,
    columns: HashMap<String, Vec<TextRow>>,
    constraints: HashMap<String, Vec<TextRow>>,
    indexes: HashMap<String, Vec<TextRow>>,
}

/// The table, columns, constraints and indexes queries, for the relations `oids` together.
async fn table_rows(conn: &dyn Connection, oids: &str) -> Result<TableRows> {
    let info = by_relation(
        conn,
        &format!(
            "SELECT c.oid::text, c.relkind::text, c.relpersistence::text, pg_get_partkeydef(c.oid),
                    (SELECT i.inhparent::regclass::text FROM pg_inherits i
                     WHERE i.inhrelid = c.oid AND c.relispartition),
                    pg_get_expr(c.relpartbound, c.oid),
                    obj_description(c.oid, 'pg_class'),
                    (SELECT s.srvname FROM pg_foreign_table ft
                     JOIN pg_foreign_server s ON s.oid = ft.ftserver WHERE ft.ftrelid = c.oid)
             FROM pg_class c WHERE c.oid IN ({oids})"
        ),
    )
    .await?;
    let columns = by_relation(
        conn,
        &format!(
            "SELECT a.attrelid::text, a.attname, {COLUMN_TYPE}, a.attnotnull,
                    pg_get_expr(d.adbin, d.adrelid), a.attidentity::text, a.attgenerated::text,
                    col_description(a.attrelid, a.attnum)
             FROM pg_attribute a
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
             WHERE a.attrelid IN ({oids}) AND a.attnum > 0 AND NOT a.attisdropped
             ORDER BY a.attrelid, a.attnum"
        ),
    )
    .await?;
    let constraints = by_relation(
        conn,
        &format!(
            "SELECT con.conrelid::text, con.conname, pg_get_constraintdef(con.oid, true)
             FROM pg_constraint con
             WHERE con.conrelid IN ({oids}) AND con.contype IN ('p', 'u', 'c', 'x', 'f')
             ORDER BY con.conrelid, con.contype = 'p' DESC, con.contype = 'f', con.conname"
        ),
    )
    .await?;
    // Indexes that back a constraint are created by it.
    let indexes = by_relation(
        conn,
        &format!(
            "SELECT i.indrelid::text, pg_get_indexdef(i.indexrelid)
             FROM pg_index i JOIN pg_class ic ON ic.oid = i.indexrelid
             WHERE i.indrelid IN ({oids}) AND NOT EXISTS (
                 SELECT 1 FROM pg_constraint con
                 WHERE con.conindid = i.indexrelid AND con.conrelid = i.indrelid)
             ORDER BY i.indrelid, ic.relname"
        ),
    )
    .await?;
    Ok(TableRows {
        info,
        columns,
        constraints,
        indexes,
    })
}

/// The DDL of tables and foreign tables, with the same four catalog queries for all of them
/// (and, on `TimescaleDB` and YugabyteDB, a few more a table).
async fn tables_ddl(conn: &dyn Connection, objects: &[ObjectRef<'_>]) -> Vec<Result<String>> {
    if objects.is_empty() {
        return Vec::new();
    }
    let read = async {
        let oids = relation_oids(conn, objects).await?;
        let list: Vec<&str> = oids.values().map(String::as_str).collect();
        let rows = if list.is_empty() {
            None
        } else {
            Some(table_rows(conn, &list.join(", ")).await?)
        };
        let timescale = !crate::is_cockroach(conn) && has_timescale(conn).await?;
        Ok::<_, Error>((oids, rows, timescale))
    };
    let (oids, rows, timescale) = match read.await {
        Ok(read) => read,
        Err(e) => return failed(objects, &e),
    };
    let mut out = Vec::with_capacity(objects.len());
    for o in objects {
        let found = oids
            .get(&(o.schema.to_owned(), o.name.to_owned()))
            .zip(rows.as_ref());
        out.push(match found {
            Some((oid, rows)) => table(conn, o, (oid, rows), timescale).await,
            None => Err(not_found(o)),
        });
    }
    out
}

/// The rows of relation `oid` in `map`, none when it has none.
fn of<'a>(map: &'a HashMap<String, Vec<TextRow>>, oid: &str) -> &'a [TextRow] {
    map.get(oid).map_or(&[], Vec::as_slice)
}

/// The DDL of table `o` from the rows read about it.
async fn table(
    conn: &dyn Connection,
    o: &ObjectRef<'_>,
    (oid, rows): (&str, &TableRows),
    timescale: bool,
) -> Result<String> {
    let name = qualified(o.schema, o.name);
    let info = of(&rows.info, oid).first().ok_or_else(|| not_found(o))?;
    let columns = of(&rows.columns, oid);
    let indexes: Vec<String> = of(&rows.indexes, oid)
        .iter()
        .map(|i| format!("{};", i.text(0)))
        .collect();
    let mut create = create_table(&name, info, columns, of(&rows.constraints, oid));
    if crate::is_yugabyte(conn) && info.opt(3).is_none() {
        let rel = relation(o.schema, o.name);
        let sharding = yugabyte_sharding(conn, &rel).await.unwrap_or((None, None));
        create = with_sharding(create, sharding);
    }
    let hypertable = if timescale {
        // What the TimescaleDB catalog cannot tell is left out, not the whole DDL.
        hypertable(conn, o, &name).await.unwrap_or_default()
    } else {
        String::new()
    };
    let sections = [
        create,
        hypertable,
        indexes.join(
            "
",
        ),
        comments(&name, info, columns),
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

/// The `CREATE TABLE` statement `create` with YugabyteDB's `key` (in place of the plain
/// `PRIMARY KEY (…)` constraint) and `split` clause.
fn with_sharding(create: String, (key, split): (Option<String>, Option<String>)) -> String {
    let mut sql = create;
    if let Some(key) = key
        && let Some(at) = sql.find(" PRIMARY KEY (")
    {
        let start = at + 1;
        if let Some(end) = sql[start..].find(')') {
            sql.replace_range(start..=start + end, &key);
        }
    }
    if let Some(split) = split {
        let body = sql.trim_end_matches(';').to_owned();
        sql = format!("{body}\n{split};");
    }
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
            // A hypertable's rows and sizes are those of its chunks: its own table is empty.
            let hyper = match o.kind {
                ObjectKind::Table if !crate::is_cockroach(conn) => {
                    hypertable_sizes(conn, o).await.unwrap_or(None)
                }
                _ => None,
            };
            let r = hyper.as_ref().unwrap_or(&r);
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
