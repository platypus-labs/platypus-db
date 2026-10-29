//! A schema as one SQL file that runs again on the same kind of database: the database's own
//! DDL of its sequences and tables, the rows as `INSERT`s, then the foreign keys, the
//! numbering of identity columns and sequences, and the views, routines and triggers.
//!
//! Tables come in the order of their foreign keys. Where the database adds a foreign key to a
//! table that exists (`ALTER TABLE … ADD`), the keys leave the `CREATE TABLE` and come after
//! the rows, so that a table that refers to itself or a cycle of tables loads too.

use std::collections::{HashMap, HashSet};
use std::io::Write;

use futures::StreamExt;
use platypus_core::{
    Connection, ExecOptions, ExecResult, ObjectKind, ObjectMeta, ObjectRef, TableDetails,
};
use platypus_sql::Dialect;
use platypus_sql::dml::{Column, Value, ValueKind, restore_inserts};
use platypus_sql::generated::{Generated, generated_columns};

use crate::Compression;
use crate::import::{ImportError, query_rows, rows_text};
use crate::migrate::End;

/// What a dump wrote.
#[derive(Debug, Default)]
pub struct Dumped {
    pub tables: usize,
    pub rows: u64,
    /// What was left out, with why.
    pub skipped: Vec<String>,
}

/// The kinds of object written after the tables, in this order.
const OBJECT_KINDS: [ObjectKind; 5] = [
    ObjectKind::Function,
    ObjectKind::Procedure,
    ObjectKind::View,
    ObjectKind::MaterializedView,
    ObjectKind::Trigger,
];

/// Writes `schema` of `conn` to `out`: its DDL and, with `data`, its rows; with `only`, the
/// tables, sequences, views and routines of those names alone (and the triggers of those
/// tables). `progress` gets the rows written so far and returns false to stop.
///
/// # Errors
/// When the database cannot describe its tables, reading rows fails, or writing fails.
pub async fn dump(
    (conn, dialect, schema): End<'_>,
    out: &mut (dyn Write + Send),
    (data, only): (bool, Option<&[String]>),
    mut progress: impl FnMut(u64) -> bool,
) -> Result<Dumped, ImportError> {
    let server = conn.server_info();
    let mut w = Writer { out, dialect };
    w.line(&format!(
        "-- Schema {schema} of {} {}, dumped by PlatypusDB.",
        server.product, server.version
    ))?;
    w.line("-- Runs on the same kind of database: Run SQL file, or the database's own client.")?;
    // The rows of a table may come before those they refer to: MySQL checks no key until
    // the end, SQLite checks them at the commit (or the end of each statement).
    match dialect {
        Dialect::MySql => w.statement("SET FOREIGN_KEY_CHECKS = 0")?,
        Dialect::Sqlite => w.statement("PRAGMA defer_foreign_keys = ON")?,
        _ => {}
    }

    let Chosen {
        objects,
        tables,
        owned_by,
    } = chosen_objects(conn, dialect, schema, only).await?;
    let mut dumped = Dumped::default();
    // Enum and composite types, then the domains on them, then sequences: what columns use.
    for kind in [ObjectKind::Type, ObjectKind::Domain, ObjectKind::Sequence] {
        for o in objects.iter().filter(|o| o.kind == kind) {
            w.object(conn, schema, o.kind, &o.name, None, &mut dumped.skipped)
                .await?;
        }
    }
    let keys = create_tables(conn, &mut w, schema, &tables).await?;
    for sql in &owned_by {
        w.statement(sql)?;
    }
    dumped.tables = tables.len();

    if data {
        for (name, details) in &tables {
            // DuckDB checks a key against the rows of the statements before: a table that
            // refers to itself takes a row a statement.
            let one_by_one = dialect == Dialect::DuckDb
                && details.foreign_keys.iter().any(|fk| &fk.ref_table == name);
            let numbered = rows(
                conn,
                &mut w,
                (schema, name, details, one_by_one),
                &mut dumped.rows,
                &mut progress,
            )
            .await?;
            for column in numbered {
                if let Some(sql) = renumber(dialect, schema, name, &column) {
                    w.statement(&sql)?;
                }
            }
        }
        for o in objects.iter().filter(|o| o.kind == ObjectKind::Sequence) {
            if let Some(sql) = sequence_value(conn, dialect, schema, &o.name).await {
                w.statement(&sql)?;
            }
        }
    }
    for key in &keys {
        w.statement(key)?;
    }
    let names: HashSet<&str> = tables.iter().map(|(n, _)| n.as_str()).collect();
    for kind in OBJECT_KINDS {
        for o in objects.iter().filter(|o| o.kind == kind) {
            // A trigger of a table that is not in the dump has nothing to fire on.
            if kind == ObjectKind::Trigger
                && !o.parent.as_deref().is_some_and(|p| names.contains(p))
            {
                continue;
            }
            w.object(
                conn,
                schema,
                o.kind,
                &o.name,
                o.parent.as_deref(),
                &mut dumped.skipped,
            )
            .await?;
        }
    }
    if dialect == Dialect::MySql {
        w.statement("SET FOREIGN_KEY_CHECKS = 1")?;
    }
    w.out.flush().map_err(|e| write_error(&e))?;
    Ok(dumped)
}

/// What a dump writes.
struct Chosen {
    objects: Vec<ObjectMeta>,
    /// With their details, in the order to create them.
    tables: Vec<(String, TableDetails)>,
    /// The statements that give `serial` sequences back to their columns.
    owned_by: Vec<String>,
}

/// The objects of `schema` to dump, with `only` those of these names and what they need:
/// the triggers of their tables, the types their columns use and the sequences of their
/// `serial` columns.
async fn chosen_objects(
    conn: &dyn Connection,
    dialect: Dialect,
    schema: &str,
    only: Option<&[String]>,
) -> Result<Chosen, ImportError> {
    let chosen = |name: &str| only.is_none_or(|names| names.iter().any(|n| n == name));
    let all = conn.objects(schema).await?;
    let mut tables = Vec::new();
    for o in all
        .iter()
        .filter(|o| o.kind == ObjectKind::Table && chosen(&o.name))
    {
        let details = conn.table_details(schema, &o.name).await?;
        tables.push((o.name.clone(), details));
    }
    let parents: HashMap<&str, &str> = all
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .filter_map(|o| Some((o.name.as_str(), o.parent.as_deref()?)))
        .filter(|(_, p)| tables.iter().any(|(t, _)| t == p))
        .collect();
    let tables = after_parents(ordered(tables), &parents);
    let used = |name: &str| {
        let name = name.to_ascii_lowercase();
        tables.iter().any(|(_, d)| {
            d.columns.iter().any(|c| {
                c.data_type
                    .to_ascii_lowercase()
                    .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                    .any(|word| word == name)
            })
        })
    };
    let owners = owned_sequences(conn, dialect, schema).await;
    let objects: Vec<ObjectMeta> = all
        .iter()
        .filter(|o| match o.kind {
            ObjectKind::Trigger => true,
            ObjectKind::Type | ObjectKind::Domain => only.is_none() || used(&o.name),
            // An identity column's sequence comes with its table.
            ObjectKind::Sequence => match owners.get(&o.name) {
                Some(owner) if owner.identity => false,
                Some(owner) => chosen(&owner.table) || chosen(&o.name),
                None => chosen(&o.name),
            },
            _ => chosen(&o.name),
        })
        .cloned()
        .collect();
    let owned_by = objects
        .iter()
        .filter_map(|o| {
            let owner = owners.get(&o.name)?;
            tables.iter().any(|(t, _)| *t == owner.table).then(|| {
                format!(
                    "ALTER SEQUENCE {} OWNED BY {}.{}",
                    dialect.qualified(Some(schema), &o.name),
                    dialect.qualified(Some(schema), &owner.table),
                    dialect.quote_ident(&owner.column)
                )
            })
        })
        .collect();
    Ok(Chosen {
        objects,
        tables,
        owned_by,
    })
}

/// A sequence that belongs to a column.
struct Owner {
    table: String,
    column: String,
    /// An identity column's, which the table's DDL makes, rather than a `serial` one's.
    identity: bool,
}

/// PostgreSQL's sequences that belong to a column, by name.
async fn owned_sequences(
    conn: &dyn Connection,
    dialect: Dialect,
    schema: &str,
) -> HashMap<String, Owner> {
    if dialect != Dialect::Postgres {
        return HashMap::new();
    }
    let sql = format!(
        "SELECT s.relname, t.relname, a.attname, d.deptype::text FROM pg_class s \
         JOIN pg_namespace n ON n.oid = s.relnamespace \
         JOIN pg_depend d ON d.classid = 'pg_class'::regclass AND d.objid = s.oid \
           AND d.refclassid = 'pg_class'::regclass AND d.deptype IN ('a', 'i') \
         JOIN pg_class t ON t.oid = d.refobjid \
         JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = d.refobjsubid \
         WHERE s.relkind = 'S' AND n.nspname = {}",
        dialect.quote_literal(schema)
    );
    query_rows(conn, &sql)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| {
            let mut row = row.into_iter();
            let (name, table, column) = (row.next()??, row.next()??, row.next()??);
            let identity = row.next()?? == "i";
            Some((
                name,
                Owner {
                    table,
                    column,
                    identity,
                },
            ))
        })
        .collect()
}

/// Partitions after the table they are a partition of, which creates them.
fn after_parents(
    tables: Vec<(String, TableDetails)>,
    parents: &HashMap<&str, &str>,
) -> Vec<(String, TableDetails)> {
    let depth = |name: &str| {
        let (mut at, mut depth): (&str, usize) = (name, 0);
        while let Some(parent) = parents.get(at) {
            at = parent;
            depth += 1;
            if depth > parents.len() {
                break;
            }
        }
        depth
    };
    let mut tables = tables;
    tables.sort_by_key(|(name, _)| depth(name));
    tables
}

/// The DDL of `tables`; the foreign keys taken out of it, as the `ALTER TABLE`s that add them
/// after the rows.
async fn create_tables(
    conn: &dyn Connection,
    w: &mut Writer<'_>,
    schema: &str,
    tables: &[(String, TableDetails)],
) -> Result<Vec<String>, ImportError> {
    let dialect = w.dialect;
    let mut keys = Vec::new();
    for (name, _) in tables {
        let object = ObjectRef {
            schema,
            name,
            kind: ObjectKind::Table,
            parent: None,
        };
        let ddl = conn.object_ddl(&object).await?;
        let (ddl, foreign) = if adds_foreign_keys(dialect) {
            without_foreign_keys(&ddl)
        } else {
            (ddl, Vec::new())
        };
        w.ddl(&ddl)?;
        let table = dialect.qualified(Some(schema), name);
        keys.extend(
            foreign
                .into_iter()
                .map(|k| format!("ALTER TABLE {table} ADD {k}")),
        );
    }
    Ok(keys)
}

/// The rows of `schema.table`, described by `details`, as `INSERT`s; the columns the database
/// numbers, which take the rows' values with an override and count on from the largest after.
async fn rows(
    conn: &dyn Connection,
    w: &mut Writer<'_>,
    (schema, table, details, one_by_one): (&str, &str, &TableDetails, bool),
    written: &mut u64,
    progress: &mut impl FnMut(u64) -> bool,
) -> Result<Vec<String>, ImportError> {
    let dialect = w.dialect;
    let generated = generated_columns(conn, dialect, schema, table)
        .await
        .unwrap_or_default();
    let how = |name: &str| {
        generated
            .iter()
            .find(|g| g.name == name)
            .map(|g| g.generated)
    };
    // A computed column is never written: the database works it out again.
    let names: Vec<&str> = details
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .filter(|n| how(n) != Some(Generated::Computed))
        .collect();
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let identity: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(_, n)| how(n) == Some(Generated::Identity))
        .map(|(i, _)| i)
        .collect();
    let numbered: Vec<String> = names
        .iter()
        .filter(|n| matches!(how(n), Some(Generated::Identity | Generated::Numbered)))
        .map(|n| (*n).to_owned())
        .collect();
    let quoted: Vec<_> = names.iter().map(|n| dialect.quote_ident(n)).collect();
    // PostgreSQL writes every value as its own text, which it reads back as the column's
    // type: arrays, composites, ranges, money and the rest included.
    let select: Vec<String> = if dialect == Dialect::Postgres {
        quoted.iter().map(|q| format!("{q}::text")).collect()
    } else {
        quoted.iter().map(ToString::to_string).collect()
    };
    let qualified = dialect.qualified(Some(schema), table);
    // A PostgreSQL table's own rows: those of its partitions and children are theirs.
    let only = if dialect == Dialect::Postgres {
        "ONLY "
    } else {
        ""
    };
    let sql = format!("SELECT {} FROM {only}{qualified}", select.join(", "));
    let ExecResult::Rows(mut stream) = conn.execute(&sql, &ExecOptions::default()).await? else {
        return Ok(numbered);
    };
    while let Some(batch) = stream.next().await {
        let batch = batch?;
        let columns: Vec<Column<'_>> = names
            .iter()
            .zip(batch.schema().fields())
            .map(|(name, field)| Column {
                name,
                kind: if dialect == Dialect::Postgres {
                    ValueKind::Text
                } else {
                    ValueKind::of(field.data_type())
                },
                default: None,
            })
            .collect();
        let text = rows_text(&batch)?;
        let values: Vec<Vec<(usize, Value<'_>)>> = text
            .iter()
            .map(|row| {
                row.iter()
                    .enumerate()
                    .map(|(i, v)| (i, v.as_deref().map_or(Value::Null, Value::Text)))
                    .collect()
            })
            .collect();
        let rows_each = if one_by_one { 1 } else { values.len().max(1) };
        for chunk in values.chunks(rows_each) {
            for sql in restore_inserts(dialect, &qualified, &columns, chunk, &identity)? {
                w.statement(&sql)?;
            }
        }
        *written += batch.num_rows() as u64;
        if !progress(*written) {
            return Err(ImportError::Cancelled);
        }
    }
    Ok(numbered)
}

/// The statement that numbers on from the largest value of `column` after the rows were
/// put back with theirs; `None` where the database does it by itself.
fn renumber(dialect: Dialect, schema: &str, table: &str, column: &str) -> Option<String> {
    let t = dialect.qualified(Some(schema), table);
    let c = dialect.quote_ident(column);
    match dialect {
        Dialect::Postgres => Some(format!(
            "SELECT setval(pg_get_serial_sequence({}, {}), COALESCE(MAX({c}), 0) + 1, false) \
             FROM {t}",
            dialect.quote_literal(&t),
            dialect.quote_literal(column)
        )),
        // MySQL's AUTO_INCREMENT, SQL Server's IDENTITY and SQLite's rowid count on from the
        // largest value inserted.
        _ => None,
    }
}

/// A sequence's position, so that it goes on where it was, where its DDL tells only where it
/// started (PostgreSQL, SQL Server, MariaDB).
async fn sequence_value(
    conn: &dyn Connection,
    dialect: Dialect,
    schema: &str,
    name: &str,
) -> Option<String> {
    let sequence = dialect.qualified(Some(schema), name);
    let sql = match dialect {
        Dialect::Postgres => format!("SELECT last_value, is_called FROM {sequence}"),
        // NULL before its first value, which the DDL's start gives.
        Dialect::SqlServer => format!(
            "SELECT CAST(CAST(last_used_value AS bigint) + CAST(increment AS bigint) AS varchar(40)) \
             FROM sys.sequences WHERE object_id = OBJECT_ID({})",
            dialect.quote_literal(&sequence)
        ),
        Dialect::MySql => format!("SELECT next_not_cached_value FROM {sequence}"),
        _ => return None,
    };
    let row = query_rows(conn, &sql).await.ok()?.into_iter().next()?;
    let value = row.first().cloned().flatten()?;
    match dialect {
        Dialect::Postgres => {
            let called = row.get(1).cloned().flatten().is_some_and(|c| c == "true");
            Some(format!(
                "SELECT setval({}, {value}, {called})",
                dialect.quote_literal(&sequence)
            ))
        }
        _ => Some(format!("ALTER SEQUENCE {sequence} RESTART WITH {value}")),
    }
}

/// Tables after those their foreign keys refer to; in a cycle, the first goes anyway.
fn ordered(tables: Vec<(String, TableDetails)>) -> Vec<(String, TableDetails)> {
    let mut pending = tables;
    let mut done: Vec<(String, TableDetails)> = Vec::with_capacity(pending.len());
    while !pending.is_empty() {
        let ready = |(name, details): &(String, TableDetails)| {
            details.foreign_keys.iter().all(|fk| {
                &fk.ref_table == name
                    || done.iter().any(|(d, _)| *d == fk.ref_table)
                    || !pending.iter().any(|(p, _)| *p == fk.ref_table)
            })
        };
        let next = pending.iter().position(ready).unwrap_or(0);
        done.push(pending.remove(next));
    }
    done
}

/// Whether the database adds a foreign key to a table that exists.
fn adds_foreign_keys(dialect: Dialect) -> bool {
    matches!(
        dialect,
        Dialect::Postgres | Dialect::MySql | Dialect::SqlServer | Dialect::Oracle
    )
}

/// `ddl` without the `CONSTRAINT … FOREIGN KEY …` lines of its `CREATE TABLE`, and those
/// clauses. The databases write one constraint a line; the comma of the line before a
/// removed last one goes too.
fn without_foreign_keys(ddl: &str) -> (String, Vec<String>) {
    let mut lines: Vec<String> = Vec::new();
    let mut keys = Vec::new();
    for line in ddl.lines() {
        let trimmed = line.trim();
        let upper = trimmed.to_ascii_uppercase();
        if upper.starts_with("CONSTRAINT ") && upper.contains(" FOREIGN KEY") {
            keys.push(trimmed.trim_end_matches(',').to_owned());
            continue;
        }
        // The table's closing parenthesis: the line before it ends without a comma.
        if trimmed.starts_with(')')
            && let Some(last) = lines.last_mut()
        {
            let kept = last.trim_end().trim_end_matches(',').to_owned();
            *last = kept;
        }
        lines.push(line.to_owned());
    }
    (lines.join("\n"), keys)
}

/// Whether an object's DDL is a block whose body holds statements of its own.
fn is_block(kind: ObjectKind) -> bool {
    matches!(
        kind,
        ObjectKind::Function | ObjectKind::Procedure | ObjectKind::Trigger | ObjectKind::Package
    )
}

/// Where the statements go: one a line, ended as the dialect's scripts end them.
struct Writer<'a> {
    out: &'a mut (dyn Write + Send),
    dialect: Dialect,
}

impl Writer<'_> {
    fn line(&mut self, text: &str) -> Result<(), ImportError> {
        writeln!(self.out, "{text}").map_err(|e| write_error(&e))
    }

    /// A statement, with `;` (and `GO` in Transact-SQL, which runs routines alone).
    fn statement(&mut self, sql: &str) -> Result<(), ImportError> {
        let sql = sql.trim_end();
        if self.dialect == Dialect::SqlServer {
            writeln!(self.out, "{sql};\nGO").map_err(|e| write_error(&e))
        } else if sql.ends_with(';') {
            self.line(sql)
        } else {
            writeln!(self.out, "{sql};").map_err(|e| write_error(&e))
        }
    }

    /// DDL as the database writes it, which may hold several statements, then a blank line.
    fn ddl(&mut self, ddl: &str) -> Result<(), ImportError> {
        self.statement(ddl)?;
        self.line("")
    }

    /// A routine or trigger, whose body holds `;`: MySQL's between `DELIMITER` lines, Oracle's
    /// PL/SQL up to a `/` line; the others' scripts keep such a body whole.
    fn block(&mut self, ddl: &str) -> Result<(), ImportError> {
        let ddl = ddl.trim_end();
        match self.dialect {
            Dialect::MySql => {
                let body = ddl.trim_end_matches(';').trim_end();
                writeln!(self.out, "DELIMITER $$\n{body}$$\nDELIMITER ;\n")
                    .map_err(|e| write_error(&e))
            }
            // DBMS_METADATA's SQLTERMINATOR may have ended it already.
            Dialect::Oracle if ddl.ends_with("\n/") => self.line(&format!("{ddl}\n")),
            Dialect::Oracle => writeln!(self.out, "{ddl}\n/\n").map_err(|e| write_error(&e)),
            _ => self.ddl(ddl),
        }
    }

    /// An object's DDL; one the database cannot describe goes to `skipped`.
    async fn object(
        &mut self,
        conn: &dyn Connection,
        schema: &str,
        kind: ObjectKind,
        name: &str,
        parent: Option<&str>,
        skipped: &mut Vec<String>,
    ) -> Result<(), ImportError> {
        let object = ObjectRef {
            schema,
            name,
            kind,
            parent,
        };
        match conn.object_ddl(&object).await {
            Ok(ddl) if is_block(kind) => self.block(&ddl),
            Ok(ddl) => self.ddl(&ddl),
            Err(e) => {
                skipped.push(format!("{name}: {e}"));
                Ok(())
            }
        }
    }
}

fn write_error(e: &std::io::Error) -> ImportError {
    ImportError::Plan(format!("cannot write the dump: {e}"))
}

/// The file a dump goes to, compressed as its extension says (`.gz`, `.zst`). It is written
/// next to it, as `<name>.part`, and takes the name when finished: a dump that fails or
/// stops leaves the file it would have replaced as it was.
#[derive(Debug)]
pub struct DumpFile {
    out: Encoder,
    partial: std::path::PathBuf,
    target: std::path::PathBuf,
}

enum Encoder {
    Plain(std::io::BufWriter<std::fs::File>),
    Gzip(flate2::write::GzEncoder<std::io::BufWriter<std::fs::File>>),
    Zstd(zstd::Encoder<'static, std::io::BufWriter<std::fs::File>>),
}

impl DumpFile {
    /// Starts the file for `path`.
    ///
    /// # Errors
    /// When it cannot be created.
    pub fn create(path: &std::path::Path) -> std::io::Result<Self> {
        let mut partial = path.as_os_str().to_owned();
        partial.push(".part");
        let partial = std::path::PathBuf::from(partial);
        let file = std::fs::File::create(&partial)?;
        let out = std::io::BufWriter::with_capacity(1 << 20, file);
        let out = match Compression::of_path(path) {
            Compression::Gzip => Encoder::Gzip(flate2::write::GzEncoder::new(
                out,
                flate2::Compression::default(),
            )),
            Compression::Zstd => match zstd::Encoder::new(out, 3) {
                Ok(zst) => Encoder::Zstd(zst),
                Err(e) => {
                    let _ = std::fs::remove_file(&partial);
                    return Err(e);
                }
            },
            Compression::None => Encoder::Plain(out),
        };
        Ok(Self {
            out,
            partial,
            target: path.to_owned(),
        })
    }

    /// Writes what is left and a compressed file's trailer, and gives the file its name.
    ///
    /// # Errors
    /// When writing or renaming fails; the partial file is removed.
    pub fn finish(self) -> std::io::Result<()> {
        let written = match self.out {
            Encoder::Plain(mut out) => out.flush(),
            Encoder::Gzip(gz) => gz.finish().and_then(|mut out| out.flush()),
            Encoder::Zstd(zst) => zst.finish().and_then(|mut out| out.flush()),
        };
        let placed = written.and_then(|()| std::fs::rename(&self.partial, &self.target));
        if placed.is_err() {
            let _ = std::fs::remove_file(&self.partial);
        }
        placed
    }

    /// Drops what was written, leaving any file at the path as it was.
    pub fn discard(self) {
        drop(self.out);
        let _ = std::fs::remove_file(&self.partial);
    }
}

impl std::fmt::Debug for Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Plain(_) => "Plain",
            Self::Gzip(_) => "Gzip",
            Self::Zstd(_) => "Zstd",
        })
    }
}

impl Write for DumpFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &mut self.out {
            Encoder::Plain(out) => out.write(buf),
            Encoder::Gzip(out) => out.write(buf),
            Encoder::Zstd(out) => out.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match &mut self.out {
            Encoder::Plain(out) => out.flush(),
            Encoder::Gzip(out) => out.flush(),
            Encoder::Zstd(out) => out.flush(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dump_file_replaces_the_old_one_only_when_finished() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.sql.gz");
        std::fs::write(&path, "old").unwrap();
        let mut file = DumpFile::create(&path).unwrap();
        file.write_all(b"SELECT 1;").unwrap();
        file.discard();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        let mut file = DumpFile::create(&path).unwrap();
        file.write_all(b"SELECT 1;").unwrap();
        file.finish().unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(
            &mut flate2::read::GzDecoder::new(std::fs::File::open(&path).unwrap()),
            &mut text,
        )
        .unwrap();
        assert_eq!(text, "SELECT 1;");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn foreign_keys_leave_the_create_table() {
        let pg = "CREATE TABLE s.orders (\n    id integer NOT NULL,\n    customer_id integer,\n    CONSTRAINT orders_pkey PRIMARY KEY (id),\n    CONSTRAINT orders_customer_fk FOREIGN KEY (customer_id) REFERENCES s.customers(id) ON DELETE CASCADE\n);\nCREATE INDEX orders_idx ON s.orders USING btree (customer_id);";
        let (ddl, keys) = without_foreign_keys(pg);
        assert_eq!(
            ddl,
            "CREATE TABLE s.orders (\n    id integer NOT NULL,\n    customer_id integer,\n    CONSTRAINT orders_pkey PRIMARY KEY (id)\n);\nCREATE INDEX orders_idx ON s.orders USING btree (customer_id);"
        );
        assert_eq!(
            keys,
            [
                "CONSTRAINT orders_customer_fk FOREIGN KEY (customer_id) REFERENCES s.customers(id) ON DELETE CASCADE"
            ]
        );
        let mysql = "CREATE TABLE `t` (\n  `id` int NOT NULL,\n  `boss` int DEFAULT NULL,\n  PRIMARY KEY (`id`),\n  KEY `boss` (`boss`),\n  CONSTRAINT `t_boss` FOREIGN KEY (`boss`) REFERENCES `t` (`id`)\n) ENGINE=InnoDB";
        let (ddl, keys) = without_foreign_keys(mysql);
        assert!(
            ddl.contains("  KEY `boss` (`boss`)\n) ENGINE=InnoDB"),
            "{ddl}"
        );
        assert_eq!(
            keys,
            ["CONSTRAINT `t_boss` FOREIGN KEY (`boss`) REFERENCES `t` (`id`)"]
        );
    }

    #[test]
    fn tables_come_after_those_they_refer_to() {
        let table = |name: &str, refs: &[&str]| {
            let details = TableDetails {
                foreign_keys: refs
                    .iter()
                    .map(|r| platypus_core::ForeignKeyMeta {
                        name: format!("{name}_{r}"),
                        columns: vec![],
                        ref_schema: String::new(),
                        ref_table: (*r).to_owned(),
                        ref_columns: vec![],
                    })
                    .collect(),
                ..TableDetails::default()
            };
            (name.to_owned(), details)
        };
        let order: Vec<_> = ordered(vec![
            table("lines", &["orders", "products"]),
            table("orders", &["customers"]),
            table("staff", &["staff"]),
            table("customers", &[]),
            table("products", &[]),
        ])
        .into_iter()
        .map(|(n, _)| n)
        .collect();
        assert_eq!(order, ["staff", "customers", "orders", "products", "lines"]);
    }
}
