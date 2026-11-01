//! The script that makes one schema like another: tables created, altered with [`script`] and
//! dropped, in an order the foreign keys allow.

use std::borrow::Cow;

use platypus_sql::Dialect;

use crate::ddl::{
    Keys, add_foreign_key, phased, same_check, taken_names, transactional, warn_one_by_one,
};
use crate::{ColumnDesign, ConstraintDesign, ForeignKeyDesign, Script, TableDesign};

/// The script that turns the tables `current` of `schema` into `wanted`, read from
/// `wanted_schema` (possibly of another database). Tables are matched by name, and their
/// parts too: a renamed column is a dropped column and a new one. The parts a database
/// names itself (`SYS_C0012`, `UQ__orders__3213E83F`) are matched by what they are.
///
/// Where tables are altered in place, the foreign keys that change are dropped first and
/// added last, so that what they reference (a column or a table the script adds) is there,
/// and the tables gone are dropped before the others change.
pub fn compare(
    dialect: Dialect,
    schema: &str,
    wanted_schema: &str,
    wanted: &[TableDesign],
    current: &[TableDesign],
) -> Result<Script, String> {
    // The history tables of system-versioned tables come and go with them.
    let (wanted, current) = (&*without_history(wanted), &*without_history(current));
    let in_place = keys_in_place(dialect);
    let mut keys = Keys::default();
    let mut altered = Script::default();
    let mut sequences = Vec::new();
    for old in current {
        if let Some(w) = find(wanted, &old.name) {
            let design = matched(
                dialect,
                w,
                Some(old),
                (schema, wanted_schema),
                &mut sequences,
            );
            add(
                &mut altered,
                phased(dialect, schema, Some(old), &design, Some(&mut keys))?,
            );
        }
    }
    let dropped = drop_gone(dialect, schema, wanted, current)?;
    let mut out = Script {
        transactional: transactional(dialect),
        ..Script::default()
    };
    out.statements.append(&mut keys.dropped);
    let dropped = if in_place {
        add(&mut out, dropped);
        Script::default()
    } else {
        dropped
    };
    let later = create_new(
        &mut out,
        dialect,
        (schema, wanted_schema),
        wanted,
        current,
        &mut sequences,
    )?;
    // First, for the tables created and the defaults set to take their values.
    let created = missing_sequences(schema, sequences, wanted, current);
    out.statements.splice(0..0, created.statements);
    out.warnings.extend(created.warnings);
    // Before all, for the columns of the tables created and altered to take them.
    if dialect == Dialect::Postgres {
        let types = missing_types((schema, wanted_schema), wanted, current);
        out.statements.splice(0..0, types.statements);
        out.warnings.extend(types.warnings);
    }
    add(&mut out, altered);
    out.statements.extend(later);
    out.statements.append(&mut keys.added);
    add(&mut out, dropped);
    warn_one_by_one(dialect, &mut out);
    Ok(out)
}

/// The script for the edits made on an ER diagram: `designs` are the wanted designs of the
/// tables created or changed (replacing the table of the same name in `current`, or added to
/// it), and `dropped` the tables to drop. Everything else stays as it is.
pub fn edited_script(
    dialect: Dialect,
    schema: &str,
    current: &[TableDesign],
    designs: &[TableDesign],
    dropped: &[String],
) -> Result<Script, String> {
    let mut wanted: Vec<TableDesign> = current
        .iter()
        .filter(|t| !dropped.contains(&t.name))
        .map(|t| find(designs, &t.name).unwrap_or(t).clone())
        .collect();
    for design in designs {
        if find(current, &design.name).is_none() && !dropped.contains(&design.name) {
            wanted.push(design.clone());
        }
    }
    // A dropped table takes the keys that reference it with it, also those of tables left
    // unedited: they are dropped before it is.
    for t in &mut wanted {
        t.foreign_keys.retain(|f| {
            let here = f.ref_schema.is_empty() || f.ref_schema == schema;
            !(here && dropped.contains(&f.ref_table))
        });
    }
    compare(dialect, schema, schema, &wanted, current)
}

/// Whether `dialect` adds and drops foreign keys with `ALTER TABLE`: SQLite and DuckDB
/// create a table again to change its keys, which they keep inline.
fn keys_in_place(dialect: Dialect) -> bool {
    !matches!(dialect, Dialect::Sqlite | Dialect::DuckDb)
}

fn find<'a>(tables: &'a [TableDesign], name: &str) -> Option<&'a TableDesign> {
    tables.iter().find(|t| t.name == name)
}

/// The history table of `table` when it is system-versioned (SQL Server).
fn history(table: &TableDesign) -> Option<&str> {
    table.system_time.as_ref()?.history.as_deref()
}

/// `tables` without the history tables of the system-versioned ones: the database creates,
/// changes and keeps them with their table.
fn without_history(tables: &[TableDesign]) -> Cow<'_, [TableDesign]> {
    let histories: Vec<&str> = tables.iter().filter_map(history).collect();
    if histories.is_empty() {
        return Cow::Borrowed(tables);
    }
    Cow::Owned(
        tables
            .iter()
            .filter(|t| !histories.contains(&t.name.as_str()))
            .cloned()
            .collect(),
    )
}

/// The script that drops the tables of `current` that `wanted` has not, those that
/// reference others first.
fn drop_gone(
    dialect: Dialect,
    schema: &str,
    wanted: &[TableDesign],
    current: &[TableDesign],
) -> Result<Script, String> {
    let gone: Vec<TableDesign> = current
        .iter()
        .filter(|c| find(wanted, &c.name).is_none())
        .cloned()
        .collect();
    let mut out = Script::default();
    let (ordered, cycle) = by_references(&gone, true);
    for table in &cycle {
        add(
            &mut out,
            phased(dialect, schema, Some(table), &without_keys(table), None)?,
        );
    }
    for table in ordered.into_iter().chain(cycle) {
        let name = dialect.qualified(Some(schema), &table.name);
        // A system-versioned table is dropped once it is not, then its history table.
        let history = history(table).filter(|_| dialect == Dialect::SqlServer);
        if history.is_some() {
            out.statements
                .push(format!("ALTER TABLE {name} SET (SYSTEM_VERSIONING = OFF)"));
        }
        out.statements.push(format!("DROP TABLE {name}"));
        for dropped in std::iter::once(table.name.as_str()).chain(history) {
            out.warnings
                .push(format!("Drops the table {dropped}: its rows are lost."));
        }
        if let Some(history) = history {
            out.statements.push(format!(
                "DROP TABLE {}",
                dialect.qualified(Some(schema), history)
            ));
        }
    }
    Ok(out)
}

/// Adds to `out` the tables of `wanted` that `current` has not, each after those it
/// references. Where tables are altered in place, the keys that wait for the changes of the
/// tables there (and those of a cycle) are returned instead, to add last.
fn create_new(
    out: &mut Script,
    dialect: Dialect,
    (schema, wanted_schema): (&str, &str),
    wanted: &[TableDesign],
    current: &[TableDesign],
    sequences: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    let in_place = keys_in_place(dialect);
    let mut later = Vec::new();
    let mut new = Vec::new();
    for w in wanted.iter().filter(|w| find(current, &w.name).is_none()) {
        if dialect == Dialect::ClickHouse
            && let Some(path) = w.options.as_deref().and_then(shared_keeper_path)
        {
            out.warnings.push(format!(
                "The table {} is not created: its engine replicates through the keeper path {path}, which a copy would share with the source table and join its replicas. Create it by hand with a path of its own (one with the uuid or database macro).",
                w.name
            ));
            continue;
        }
        let mut t = matched(dialect, w, None, (schema, wanted_schema), sequences);
        if in_place {
            let (now, after): (Vec<_>, Vec<_>) = std::mem::take(&mut t.foreign_keys)
                .into_iter()
                .partition(|f| f.ref_schema != schema || find(current, &f.ref_table).is_none());
            t.foreign_keys = now;
            let mut taken = taken_names(&t);
            later.extend(
                after
                    .iter()
                    .flat_map(|f| add_foreign_key(dialect, schema, &t.name, f, &mut taken)),
            );
        }
        new.push(t);
    }
    let (ordered, cycle) = by_references(&new, false);
    for table in ordered {
        add(out, phased(dialect, schema, None, table, None)?);
    }
    let bare: Vec<TableDesign> = cycle.iter().map(|t| without_keys(t)).collect();
    for table in &bare {
        add(out, phased(dialect, schema, None, table, None)?);
    }
    let mut keys = Keys::default();
    for (table, bare) in cycle.iter().zip(&bare) {
        let mut keyed = bare.clone();
        keyed.foreign_keys.clone_from(&table.foreign_keys);
        let keys = in_place.then_some(&mut keys);
        add(out, phased(dialect, schema, Some(bare), &keyed, keys)?);
    }
    keys.added.append(&mut later);
    Ok(keys.added)
}

/// The keeper path of a ClickHouse `Replicated*` engine in the table options `options`
/// (`ENGINE = ReplicatedMergeTree('/clickhouse/tables/t', '{replica}') ORDER BY id`) that a
/// copy of the table in another database would share: one naming neither the table's
/// `{uuid}` nor its `{database}`. `None` for other engines, and one without arguments, which
/// takes the server's default path.
fn shared_keeper_path(options: &str) -> Option<String> {
    let engine = options
        .trim_start()
        .strip_prefix("ENGINE")?
        .trim_start()
        .strip_prefix('=')?
        .trim_start();
    let name_end = engine
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(engine.len());
    if !engine[..name_end].starts_with("Replicated") {
        return None;
    }
    let quoted = engine[name_end..]
        .trim_start()
        .strip_prefix('(')?
        .trim_start()
        .strip_prefix('\'')?;
    let mut path = String::new();
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => path.extend(chars.next()),
            '\'' => break,
            c => path.push(c),
        }
    }
    (!path.contains("{uuid}") && !path.contains("{database}")).then_some(path)
}

/// Whether the database gave the constraint `name` itself: SQLite's have none, SQL Server
/// names them `UQ__<table>__<hash>`, Oracle `SYS_C<number>`.
fn system_named(dialect: Dialect, name: &str) -> bool {
    match dialect {
        _ if name.is_empty() => true,
        Dialect::SqlServer => ["PK__", "UQ__", "CK__", "FK__", "DF__"]
            .iter()
            .any(|p| name.starts_with(p)),
        Dialect::Oracle => name
            .strip_prefix("SYS_C")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
        _ => false,
    }
}

/// `wanted` as a design of the table `old` of `schema`: its parts named like those of `old`
/// keep them, and its foreign keys within `wanted_schema` point into `schema`. A part the
/// database named matches the part of `old` that is the same; a new one gets no name, for
/// the database to give it its own. The sequences its defaults take values from in `schema`
/// are added to `sequences`.
fn matched(
    dialect: Dialect,
    wanted: &TableDesign,
    old: Option<&TableDesign>,
    (schema, wanted_schema): (&str, &str),
    sequences: &mut Vec<String>,
) -> TableDesign {
    let system = |name: &str| system_named(dialect, name);
    let mut t = wanted.clone();
    // A part keeps the name it had in `old` (an edit on the ER diagram renames it in place);
    // a design read from another schema has its own names there, matched by name.
    for c in &mut t.columns {
        let before = old.and_then(|o| {
            by_original(&o.columns, c.original.as_deref(), |p| &p.name)
                .or_else(|| o.columns.iter().find(|o| o.name == c.name))
        });
        c.original = before.and_then(|o| o.original.clone());
        if dialect == Dialect::Postgres {
            sequences.extend(pg_sequence_default(
                c,
                &wanted.name,
                before,
                schema,
                wanted_schema,
            ));
        }
    }
    for i in &mut t.indexes {
        let before = old.and_then(|o| {
            by_original(&o.indexes, i.original.as_deref(), |p| &p.name)
                .or_else(|| o.indexes.iter().find(|o| o.name == i.name))
        });
        i.original = before.and_then(|o| o.original.clone());
    }
    for f in &mut t.foreign_keys {
        if f.ref_schema == wanted_schema {
            schema.clone_into(&mut f.ref_schema);
        }
        let generated = system(&f.name);
        let same = |o: &&ForeignKeyDesign| {
            if generated {
                system(&o.name)
                    && (&o.columns, &o.ref_schema, &o.ref_table, &o.ref_columns)
                        == (&f.columns, &f.ref_schema, &f.ref_table, &f.ref_columns)
            } else {
                o.name == f.name
            }
        };
        let before = old.and_then(|o| o.foreign_keys.iter().find(same));
        if generated {
            f.name = before.map(|o| o.name.clone()).unwrap_or_default();
        }
        f.original = before.and_then(|o| o.original.clone());
    }
    for c in &mut t.constraints {
        let generated = system(&c.name);
        let same = |o: &&ConstraintDesign| {
            if generated {
                system(&o.name)
                    && (o.kind, &o.columns) == (c.kind, &c.columns)
                    && same_check(&o.check, &c.check)
            } else {
                o.name == c.name
            }
        };
        let before = old.and_then(|o| o.constraints.iter().find(same));
        if generated {
            c.name = before.map(|o| o.name.clone()).unwrap_or_default();
        }
        c.original = before.and_then(|o| o.original.clone());
    }
    t.primary_key_name = old.and_then(|o| o.primary_key_name.clone());
    if dialect == Dialect::Postgres && schema != wanted_schema {
        retarget_types(&mut t, schema, wanted_schema);
    }
    t
}

/// The columns of `t` with the types of `wanted_schema` they use taken from `schema` instead,
/// where the comparison creates them.
fn retarget_types(t: &mut TableDesign, schema: &str, wanted_schema: &str) {
    let d = Dialect::Postgres;
    let names: Vec<String> = t
        .types
        .iter()
        .filter(|u| u.schema == wanted_schema)
        .map(|u| u.name.clone())
        .collect();
    for name in &names {
        let from = d.qualified(Some(wanted_schema), name);
        let to = d.qualified(Some(schema), name);
        let bare = d.quote_ident(name).into_owned();
        let cast = (format!("::{bare}"), format!("::{to}"));
        for c in &mut t.columns {
            // A type on the search path is written without its schema.
            let data_type = replace_name(&c.data_type, &from, &to);
            c.data_type = match data_type.strip_prefix(&bare) {
                Some(rest) if rest.is_empty() || rest.starts_with('[') => format!("{to}{rest}"),
                _ => data_type,
            };
            for text in [&mut c.default, &mut c.generated].into_iter().flatten() {
                *text = replace_name(&replace_name(text, &from, &to), &cast.0, &cast.1);
            }
        }
    }
    for u in &mut t.types {
        u.body = names.iter().fold(u.body.clone(), |body, name| {
            replace_name(
                &body,
                &d.qualified(Some(wanted_schema), name),
                &d.qualified(Some(schema), name),
            )
        });
        if u.schema == wanted_schema {
            schema.clone_into(&mut u.schema);
        }
    }
}

/// `text` with `from` replaced by `to` where it is a whole name: not part of a longer one.
fn replace_name(text: &str, from: &str, to: &str) -> String {
    let word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '$' | '"' | '.');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(from) {
        let before = rest[..at]
            .chars()
            .next_back()
            .or_else(|| out.chars().next_back());
        let after = rest[at + from.len()..].chars().next();
        let whole = !before.is_some_and(word) && !after.is_some_and(word);
        out.push_str(&rest[..at]);
        out.push_str(if whole { to } else { from });
        rest = &rest[at + from.len()..];
    }
    out.push_str(rest);
    out
}

/// What creates the user-defined types the tables `wanted` use in `wanted_schema` that the
/// tables `current` do not in `schema`: each one in a block that leaves a type there already
/// (used by no table) as it is. A type of the same name that differs is left to the user.
fn missing_types(
    (schema, wanted_schema): (&str, &str),
    wanted: &[TableDesign],
    current: &[TableDesign],
) -> Script {
    let mut types: Vec<crate::UserType> = Vec::new();
    for t in wanted {
        let mut t = t.clone();
        if schema != wanted_schema {
            retarget_types(&mut t, schema, wanted_schema);
        }
        for u in t.types.into_iter().filter(|u| u.schema == schema) {
            if !types.iter().any(|x| x.name == u.name) {
                types.push(u);
            }
        }
    }
    // Enums and the other types before the domains that may be over them.
    types.sort_by_key(|u| (u.kind == "DOMAIN", !u.body.starts_with("AS ENUM")));
    let mut out = Script::default();
    for u in types {
        let there = current
            .iter()
            .flat_map(|t| &t.types)
            .find(|c| c.schema == schema && c.name == u.name);
        let name = Dialect::Postgres.qualified(Some(schema), &u.name);
        match there {
            Some(c) if c.kind != u.kind || c.body.trim() != u.body.trim() => {
                out.warnings.push(format!(
                    "The type {name} differs from the one compared with: change it by hand."
                ));
            }
            Some(_) => {}
            None => out.statements.push(format!(
                "DO $platypus$ BEGIN CREATE {} {name} {}; \
                 EXCEPTION WHEN duplicate_object THEN NULL; END $platypus$",
                u.kind, u.body
            )),
        }
    }
    out
}

/// The part of `parts` named `original`, if any.
fn by_original<'a, T>(
    parts: &'a [T],
    original: Option<&str>,
    name: impl Fn(&T) -> &String,
) -> Option<&'a T> {
    let original = original?;
    parts.iter().find(|p| name(p) == original)
}

/// PostgreSQL's `nextval('schema.sequence'::regclass)` default of `column` of `table`, made
/// to take its values in `schema`: the default of the column that is there already, the
/// `serial` type of a new column whose sequence is its own (`<table>_<column>_seq`), or the
/// sequence of the same name in `schema`, returned as the one the default needs there. A
/// sequence named without its schema is one of `wanted_schema` (the one read first).
fn pg_sequence_default(
    column: &mut ColumnDesign,
    table: &str,
    before: Option<&ColumnDesign>,
    schema: &str,
    wanted_schema: &str,
) -> Option<String> {
    let sequence = column.default.as_deref().and_then(nextval)?;
    if let Some(default) = before
        .and_then(|b| b.default.as_ref())
        .filter(|d| nextval(d).is_some())
    {
        column.default = Some(default.clone());
        return None;
    }
    let serial = match column.data_type.trim() {
        "integer" => Some("serial"),
        "bigint" => Some("bigserial"),
        "smallint" => Some("smallserial"),
        _ => None,
    };
    let own = sequence.name == format!("{table}_{}_seq", column.name);
    match serial {
        Some(serial) if own && before.is_none() => {
            serial.clone_into(&mut column.data_type);
            column.default = None;
            None
        }
        _ if sequence
            .schema
            .as_deref()
            .is_none_or(|s| s == wanted_schema) =>
        {
            let name = Dialect::Postgres.qualified(Some(schema), &sequence.name);
            column.default = Some(format!(
                "nextval({}::regclass)",
                Dialect::Postgres.quote_literal(&name)
            ));
            Some(sequence.name)
        }
        _ => None,
    }
}

/// `CREATE SEQUENCE` for the `sequences` of `schema` no default of the tables `current`
/// takes values from, which may not be there.
fn missing_sequences(
    schema: &str,
    mut sequences: Vec<String>,
    wanted: &[TableDesign],
    current: &[TableDesign],
) -> Script {
    // Only the columns the script keeps: dropping a column drops the sequence it owns. A
    // sequence named without its schema may be of another one on the search path.
    let kept = |t: &TableDesign, c: &ColumnDesign| {
        find(wanted, &t.name).is_some_and(|w| w.columns.iter().any(|w| w.name == c.name))
    };
    let there: Vec<Sequence> = current
        .iter()
        .flat_map(|t| t.columns.iter().filter(move |c| kept(t, c)))
        .filter_map(|c| c.default.as_deref().and_then(nextval))
        .filter(|s| s.schema.as_deref() == Some(schema))
        .collect();
    sequences.sort();
    sequences.dedup();
    let mut out = Script::default();
    for name in sequences {
        if there.iter().any(|s| s.name == name) {
            continue;
        }
        let sequence = Dialect::Postgres.qualified(Some(schema), &name);
        out.statements
            .push(format!("CREATE SEQUENCE IF NOT EXISTS {sequence}"));
        out.warnings.push(format!(
            "Creates the sequence {sequence}, which starts at 1: if its columns have values \
             already, move it past them (setval)."
        ));
    }
    out
}

/// The sequence a `nextval('..'::regclass)` default takes its values from.
struct Sequence {
    schema: Option<String>,
    name: String,
}

/// The sequence of `default` when it is `nextval('[schema.]name'::regclass)`.
fn nextval(default: &str) -> Option<Sequence> {
    let inner = default
        .trim()
        .strip_prefix("nextval('")?
        .strip_suffix("'::regclass)")?
        .replace("''", "'");
    let mut parts = split_name(&inner)?;
    let name = parts.pop()?;
    match parts.len() {
        0 => Some(Sequence { schema: None, name }),
        1 => Some(Sequence {
            schema: parts.pop(),
            name,
        }),
        _ => None,
    }
}

/// The parts of a dotted name, quotes removed: `"My Schema".seq` gives `My Schema`, `seq`.
fn split_name(text: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        let mut part = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            loop {
                match chars.next()? {
                    '"' if chars.peek() == Some(&'"') => {
                        chars.next();
                        part.push('"');
                    }
                    '"' => break,
                    c => part.push(c),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == '.' {
                    break;
                }
                part.push(c);
                chars.next();
            }
        }
        parts.push(part);
        match chars.next() {
            Some('.') => {}
            None => return Some(parts),
            Some(_) => return None,
        }
    }
}

/// `tables` with each one after the tables it references (`reverse`: before them), so they
/// can be created (dropped) in that order, and apart the tables of a cycle, which cannot.
fn by_references(tables: &[TableDesign], reverse: bool) -> (Vec<&TableDesign>, Vec<&TableDesign>) {
    let mut left: Vec<&TableDesign> = tables.iter().collect();
    let mut sorted = Vec::with_capacity(left.len());
    loop {
        let ready = left.iter().position(|t| {
            left.iter().all(|other| {
                let (from, to) = if reverse { (*other, *t) } else { (*t, *other) };
                from.name == to.name || !from.foreign_keys.iter().any(|f| f.ref_table == to.name)
            })
        });
        match ready {
            Some(i) => sorted.push(left.remove(i)),
            None => break,
        }
    }
    (sorted, left)
}

/// `table` without its foreign keys, as the database has it once created that way: the
/// tables of a cycle are created without them, then get them by `ALTER TABLE`.
fn without_keys(table: &TableDesign) -> TableDesign {
    let mut t = table.clone();
    t.foreign_keys.clear();
    for c in &mut t.columns {
        c.original.get_or_insert_with(|| c.name.clone());
    }
    for i in &mut t.indexes {
        i.original.get_or_insert_with(|| i.name.clone());
    }
    for c in &mut t.constraints {
        c.original.get_or_insert_with(|| c.name.clone());
    }
    t
}

/// Appends the statements of `part` to `out`, and the parts that each table's script repeats
/// (before, after, warnings) only once.
fn add(out: &mut Script, part: Script) {
    fn once(to: &mut Vec<String>, items: Vec<String>) {
        for s in items {
            if !to.contains(&s) {
                to.push(s);
            }
        }
    }
    once(&mut out.before, part.before);
    once(&mut out.after, part.after);
    out.statements.extend(part.statements);
    once(&mut out.warnings, part.warnings);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnDesign, ForeignKeyDesign};

    fn column(name: &str, data_type: &str) -> ColumnDesign {
        ColumnDesign {
            original: Some(name.to_owned()),
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable: true,
            ..ColumnDesign::default()
        }
    }

    fn table(name: &str, columns: Vec<ColumnDesign>, refs: &[&str], schema: &str) -> TableDesign {
        TableDesign {
            name: name.to_owned(),
            columns,
            foreign_keys: refs
                .iter()
                .map(|r| ForeignKeyDesign {
                    original: Some(format!("{name}_{r}")),
                    name: format!("{name}_{r}"),
                    columns: vec![format!("{r}_id")],
                    ref_schema: schema.to_owned(),
                    ref_table: (*r).to_owned(),
                    ref_columns: vec!["id".to_owned()],
                    ..ForeignKeyDesign::default()
                })
                .collect(),
            ..TableDesign::default()
        }
    }

    #[test]
    fn clickhouse_tables_replicated_through_the_source_path_are_not_created() {
        let engine = |e: &str| TableDesign {
            options: Some(format!("ENGINE = {e} ORDER BY id")),
            ..table("r", vec![column("id", "UInt64")], &[], "src")
        };
        let shared = engine("ReplicatedMergeTree('/clickhouse/tables/{shard}/r', '{replica}')");
        let s = compare(Dialect::ClickHouse, "dst", "src", &[shared], &[]).unwrap();
        assert!(s.statements.is_empty(), "{s:?}");
        assert!(
            s.warnings[0].starts_with(
                "The table r is not created: its engine replicates through the keeper path /clickhouse/tables/{shard}/r, which"
            ),
            "{s:?}"
        );
        // Paths of their own in the target, and the server's default path, are fine.
        for e in [
            "ReplicatedMergeTree('/clickhouse/tables/{uuid}/{shard}', '{replica}')",
            "ReplicatedReplacingMergeTree('/t/{database}/r', '{replica}', v)",
            "ReplicatedMergeTree",
            "ReplicatedMergeTree()",
            "MergeTree",
        ] {
            let s = compare(Dialect::ClickHouse, "dst", "src", &[engine(e)], &[]).unwrap();
            assert_eq!(s.statements.len(), 1, "{e}: {s:?}");
        }
        assert_eq!(
            shared_keeper_path("ENGINE = ReplicatedMergeTree('/a\\'b', 'r')"),
            Some("/a'b".to_owned())
        );
    }

    #[test]
    fn same_schemas_need_nothing() {
        let t = [table("a", vec![column("id", "integer")], &[], "public")];
        let s = compare(Dialect::Postgres, "public", "public", &t, &t).unwrap();
        assert!(s.statements.is_empty(), "{:?}", s.statements);
    }

    #[test]
    fn tables_in_a_cycle_get_their_keys_once_all_exist() {
        let wanted = [
            table(
                "a",
                vec![column("id", "integer"), column("b_id", "integer")],
                &["b"],
                "public",
            ),
            table(
                "b",
                vec![column("id", "integer"), column("a_id", "integer")],
                &["a"],
                "public",
            ),
        ];
        let s = compare(Dialect::Postgres, "public", "public", &wanted, &[]).unwrap();
        assert_eq!(s.statements.len(), 4, "{:?}", s.statements);
        assert!(
            s.statements[..2]
                .iter()
                .all(|st| st.starts_with("CREATE TABLE") && !st.contains("REFERENCES"))
        );
        assert!(
            s.statements[2..]
                .iter()
                .all(|st| st.contains("ADD CONSTRAINT") && st.contains("REFERENCES"))
        );
        let back = compare(Dialect::Postgres, "public", "public", &[], &wanted).unwrap();
        assert_eq!(back.statements.len(), 4, "{:?}", back.statements);
        assert!(
            back.statements[2..]
                .iter()
                .all(|st| st.starts_with("DROP TABLE"))
        );
    }

    #[test]
    fn unnamed_constraints_are_matched_by_their_columns() {
        use crate::ConstraintDesign;
        let mut t = table(
            "a",
            vec![column("id", "integer"), column("code", "text")],
            &[],
            "main",
        );
        t.constraints = vec![ConstraintDesign {
            original: Some("sqlite_autoindex_a_1".to_owned()),
            columns: vec!["code".to_owned()],
            ..ConstraintDesign::default()
        }];
        let tables = [t];
        let s = compare(Dialect::Sqlite, "main", "main", &tables, &tables).unwrap();
        assert_eq!(s.statements, Vec::<String>::new());
    }

    #[test]
    fn creates_alters_and_drops_in_reference_order() {
        let wanted = [
            table(
                "orders",
                vec![column("id", "integer"), column("customers_id", "integer")],
                &["customers"],
                "dev",
            ),
            table("customers", vec![column("id", "integer")], &[], "dev"),
            table(
                "items",
                vec![column("id", "bigint"), column("note", "text")],
                &[],
                "dev",
            ),
        ];
        let current = [
            table("items", vec![column("id", "integer")], &[], "prod"),
            table(
                "old_lines",
                vec![column("old_id", "integer")],
                &["old"],
                "prod",
            ),
            table("old", vec![column("id", "integer")], &[], "prod"),
        ];
        let s = compare(Dialect::Postgres, "prod", "dev", &wanted, &current).unwrap();
        let first_words: Vec<String> = s
            .statements
            .iter()
            .map(|st| st.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
            .collect();
        assert_eq!(
            first_words,
            [
                "DROP TABLE prod.old_lines",
                "DROP TABLE prod.old",
                "CREATE TABLE prod.customers",
                "CREATE TABLE prod.orders",
                "ALTER TABLE prod.items",
                "ALTER TABLE prod.items",
            ]
        );
        assert!(
            s.statements[3].contains("REFERENCES prod.customers"),
            "{}",
            s.statements[3]
        );
        let sqlite = compare(Dialect::Sqlite, "main", "main", &wanted, &current).unwrap();
        assert!(
            sqlite.statements.last().unwrap().starts_with("DROP TABLE"),
            "SQLite and DuckDB drop last: {:?}",
            sqlite.statements
        );
        assert_eq!(s.warnings.len(), 2);
    }

    fn unique(name: &str, column: &str) -> ConstraintDesign {
        ConstraintDesign {
            original: Some(name.to_owned()),
            name: name.to_owned(),
            columns: vec![column.to_owned()],
            ..ConstraintDesign::default()
        }
    }

    #[test]
    fn keys_to_columns_the_script_adds_come_last() {
        let mut accounts = table(
            "accounts",
            vec![column("id", "integer"), column("code", "varchar(10)")],
            &[],
            "src",
        );
        accounts.constraints = vec![unique("accounts_code_key", "code")];
        let mut vouchers = table(
            "vouchers",
            vec![
                column("id", "integer"),
                column("account_code", "varchar(10)"),
            ],
            &["accounts"],
            "src",
        );
        vouchers.foreign_keys[0].columns = vec!["account_code".to_owned()];
        vouchers.foreign_keys[0].ref_columns = vec!["code".to_owned()];
        let wanted = [accounts, vouchers];
        let current = [table("accounts", vec![column("id", "integer")], &[], "dst")];
        for d in [
            Dialect::Postgres,
            Dialect::MySql,
            Dialect::SqlServer,
            Dialect::Oracle,
        ] {
            let s = compare(d, "dst", "src", &wanted, &current).unwrap();
            let at = |what: &str| s.statements.iter().position(|st| st.contains(what));
            let (create, column, key) = (
                at("CREATE TABLE").unwrap(),
                at("ADD").unwrap(),
                at("REFERENCES").unwrap(),
            );
            assert!(!s.statements[create].contains("REFERENCES"), "{d:?}: {s:?}");
            assert!(
                column < key && key == s.statements.len() - 1,
                "{d:?}: {s:?}"
            );
            assert!(!s.text(d).contains("src"), "{d:?}: {s:?}");
        }
        let sqlite = compare(Dialect::Sqlite, "main", "main", &wanted, &current).unwrap();
        assert!(sqlite.statements[0].contains("REFERENCES"), "{sqlite:?}");
    }

    #[test]
    fn keys_changed_on_tables_there_are_dropped_first_and_added_last() {
        let mut old = table(
            "orders",
            vec![column("id", "integer"), column("customers_id", "integer")],
            &["customers"],
            "prod",
        );
        let customers = table("customers", vec![column("id", "integer")], &[], "prod");
        let mut new = old.clone();
        new.foreign_keys[0].on_delete = Some("CASCADE".to_owned());
        new.columns[1].data_type = "bigint".to_owned();
        old.foreign_keys[0].ref_schema = "prod".to_owned();
        let s = compare(
            Dialect::SqlServer,
            "prod",
            "prod",
            &[new, customers.clone()],
            &[old, customers],
        )
        .unwrap();
        assert!(s.statements[0].contains("DROP CONSTRAINT"), "{s:?}");
        assert!(s.statements[1].contains("ALTER COLUMN"), "{s:?}");
        assert!(s.statements[2].contains("ON DELETE CASCADE"), "{s:?}");
        assert!(s.text(Dialect::SqlServer).starts_with("BEGIN TRANSACTION;"));
    }

    #[test]
    fn constraints_the_database_named_are_matched_by_what_they_are() {
        let with = |name: &str, fk: &str, schema: &str| {
            let mut t = table(
                "customers",
                vec![column("id", "int"), column("email", "varchar(100)")],
                &[],
                "s",
            );
            t.constraints = vec![unique(name, "email")];
            t.foreign_keys = vec![ForeignKeyDesign {
                original: Some(fk.to_owned()),
                name: fk.to_owned(),
                columns: vec!["id".to_owned()],
                ref_schema: schema.to_owned(),
                ref_table: "customers".to_owned(),
                ref_columns: vec!["id".to_owned()],
                ..ForeignKeyDesign::default()
            }];
            t
        };
        let src = [with("UQ__customer__AB6E61641", "FK__customers__id__1", "s")];
        let dst = [with("UQ__customer__AB6E61642", "FK__customers__id__2", "d")];
        let s = compare(Dialect::SqlServer, "d", "s", &src, &dst).unwrap();
        assert_eq!(s.statements, Vec::<String>::new());
        let created = compare(Dialect::SqlServer, "d", "s", &src, &[]).unwrap();
        assert!(!created.statements[0].contains("UQ__"), "{created:?}");
        assert!(
            created.statements[0].contains("UNIQUE (email)"),
            "{created:?}"
        );
        let oracle = compare(
            Dialect::Oracle,
            "D",
            "S",
            &[with("SYS_C008984", "SYS_C008985", "S")],
            &[with("SYS_C009001", "SYS_C009002", "D")],
        )
        .unwrap();
        assert_eq!(oracle.statements, Vec::<String>::new());
        // A name the user gave is the constraint's: another name is another constraint.
        let named = compare(
            Dialect::Postgres,
            "d",
            "s",
            &[with("a_key", "f", "s")],
            &[with("b_key", "f", "d")],
        )
        .unwrap();
        assert_eq!(named.statements.len(), 2, "{named:?}");
    }

    #[test]
    fn oracle_scripts_are_oracle_sql() {
        let identity = ColumnDesign {
            primary_key: true,
            nullable: false,
            extra: Some("GENERATED BY DEFAULT AS IDENTITY".to_owned()),
            ..column("ID", "NUMBER")
        };
        let mut wanted = table(
            "ITEMS",
            vec![
                identity,
                ColumnDesign {
                    nullable: false,
                    default: Some("'none'".to_owned()),
                    ..column("NAME", "VARCHAR2(100)")
                },
                column("QTY", "NUMBER(10)"),
            ],
            &[],
            "S",
        );
        wanted.primary_key_name = Some("SYS_C1".to_owned());
        let created = compare(Dialect::Oracle, "D", "S", &[wanted.clone()], &[]).unwrap();
        assert_eq!(
            created.statements,
            [
                "CREATE TABLE D.ITEMS (\n  ID NUMBER GENERATED BY DEFAULT AS IDENTITY NOT NULL,\n  \
                 NAME VARCHAR2(100) DEFAULT 'none' NOT NULL,\n  QTY NUMBER(10),\n  PRIMARY KEY (ID)\n)"
            ]
        );
        let mut current = wanted.clone();
        current.columns[1].data_type = "VARCHAR2(50)".to_owned();
        current.columns[1].nullable = true;
        current.columns.pop();
        let s = compare(Dialect::Oracle, "D", "S", &[wanted], &[current]).unwrap();
        assert_eq!(
            s.statements,
            [
                "ALTER TABLE D.ITEMS MODIFY (NAME VARCHAR2(100))",
                "ALTER TABLE D.ITEMS MODIFY (NAME NOT NULL)",
                "ALTER TABLE D.ITEMS ADD (QTY NUMBER(10))",
            ]
        );
        assert!(!s.transactional);
        assert!(!s.text(Dialect::Oracle).contains("BEGIN"));
        assert_eq!(
            s.warnings,
            [
                "Oracle runs each statement by itself: if one fails, the ones before it stay applied."
            ]
        );
    }

    #[test]
    fn postgres_serial_columns_take_values_from_their_own_schema() {
        let serial = |schema: &str| ColumnDesign {
            default: Some(format!("nextval('{schema}.customers_id_seq'::regclass)")),
            nullable: false,
            ..column("id", "integer")
        };
        let wanted = [table(
            "customers",
            vec![serial("qa3s_src")],
            &[],
            "qa3s_src",
        )];
        let created = compare(Dialect::Postgres, "qa3s_dst", "qa3s_src", &wanted, &[]).unwrap();
        assert_eq!(
            created.statements,
            ["CREATE TABLE qa3s_dst.customers (\n  id serial NOT NULL\n)"]
        );
        let there = [table(
            "customers",
            vec![serial("qa3s_dst")],
            &[],
            "qa3s_dst",
        )];
        let same = compare(Dialect::Postgres, "qa3s_dst", "qa3s_src", &wanted, &there).unwrap();
        assert_eq!(same.statements, Vec::<String>::new());
        let mut other = wanted.clone();
        other[0].columns[0].default = Some("nextval('qa3s_src.shared_seq'::regclass)".to_owned());
        let moved = compare(Dialect::Postgres, "qa3s_dst", "qa3s_src", &other, &[]).unwrap();
        assert_eq!(
            moved.statements[0],
            "CREATE SEQUENCE IF NOT EXISTS qa3s_dst.shared_seq"
        );
        assert!(
            moved.statements[1].contains("nextval('qa3s_dst.shared_seq'::regclass)"),
            "{moved:?}"
        );
    }

    #[test]
    fn postgres_defaults_get_a_sequence_where_there_is_none() {
        let with_default = |default: Option<&str>| {
            table(
                "customers",
                vec![ColumnDesign {
                    default: default.map(str::to_owned),
                    nullable: false,
                    ..column("id", "integer")
                }],
                &[],
                "dst",
            )
        };
        // An existing column without a default, the sequence not its own and unqualified.
        let wanted = [with_default(Some("nextval('shared_seq'::regclass)"))];
        let plain = [with_default(None)];
        let script = compare(Dialect::Postgres, "dst", "src", &wanted, &plain).unwrap();
        assert_eq!(
            script.statements,
            [
                "CREATE SEQUENCE IF NOT EXISTS dst.shared_seq",
                "ALTER TABLE dst.customers ALTER COLUMN id SET DEFAULT nextval('dst.shared_seq'::regclass)",
            ]
        );
        assert!(
            script.warnings[0]
                .starts_with("Creates the sequence dst.shared_seq, which starts at 1"),
            "{:?}",
            script.warnings
        );
        // Another table there takes its values from it already.
        let mut there = plain.to_vec();
        there.push(table(
            "orders",
            vec![ColumnDesign {
                default: Some("nextval('dst.shared_seq'::regclass)".to_owned()),
                ..column("id", "integer")
            }],
            &[],
            "dst",
        ));
        // Dropped by the script, that table takes the sequence it owns with it.
        let script = compare(Dialect::Postgres, "dst", "src", &wanted, &there).unwrap();
        assert_eq!(
            script.statements[0],
            "CREATE SEQUENCE IF NOT EXISTS dst.shared_seq"
        );
        let mut wanted = wanted.to_vec();
        wanted.push(there[1].clone());
        let script = compare(Dialect::Postgres, "dst", "src", &wanted, &there).unwrap();
        assert_eq!(
            script.statements,
            [
                "ALTER TABLE dst.customers ALTER COLUMN id SET DEFAULT nextval('dst.shared_seq'::regclass)"
            ]
        );
    }

    #[test]
    fn sequence_names_are_read_from_nextval() {
        let s = nextval(r#"nextval('"My Schema"."a""b_seq"'::regclass)"#).unwrap();
        assert_eq!(
            (s.schema.as_deref(), s.name.as_str()),
            (Some("My Schema"), r#"a"b_seq"#)
        );
        let s = nextval("nextval('t_id_seq'::regclass)").unwrap();
        assert_eq!((s.schema, s.name.as_str()), (None, "t_id_seq"));
        assert!(nextval("now()").is_none());
    }

    /// A column as the ER diagram adds one: with no `original`.
    fn new_column(name: &str, data_type: &str) -> ColumnDesign {
        ColumnDesign {
            original: None,
            ..column(name, data_type)
        }
    }

    #[test]
    fn a_new_table_with_a_key_to_another_new_table_comes_after_it() {
        let current = [table("old", vec![column("id", "integer")], &[], "public")];
        // `child` is listed first, yet it references `parent`.
        let designs = [
            table(
                "child",
                vec![
                    new_column("id", "integer"),
                    new_column("parent_id", "integer"),
                ],
                &["parent"],
                "public",
            ),
            table("parent", vec![new_column("id", "integer")], &[], "public"),
        ];
        let s = edited_script(Dialect::Postgres, "public", &current, &designs, &[]).unwrap();
        assert_eq!(s.statements.len(), 2, "{:?}", s.statements);
        assert!(s.statements[0].starts_with("CREATE TABLE public.parent"));
        assert!(s.statements[1].starts_with("CREATE TABLE public.child"));
        assert!(s.statements[1].contains("REFERENCES public.parent"));
    }

    #[test]
    fn a_column_added_to_an_existing_table_is_an_alter() {
        let current = [table("a", vec![column("id", "integer")], &[], "public")];
        let designs = [table(
            "a",
            vec![column("id", "integer"), new_column("note", "text")],
            &[],
            "public",
        )];
        let s = edited_script(Dialect::Postgres, "public", &current, &designs, &[]).unwrap();
        assert_eq!(
            s.statements,
            ["ALTER TABLE public.a ADD COLUMN note text"],
            "{:?}",
            s.statements
        );
    }

    #[test]
    fn a_dropped_table_nothing_references_is_dropped_alone() {
        let current = [
            table("a", vec![column("id", "integer")], &[], "public"),
            table("b", vec![column("id", "integer")], &[], "public"),
        ];
        let s = edited_script(
            Dialect::Postgres,
            "public",
            &current,
            &[],
            &["b".to_owned()],
        )
        .unwrap();
        assert_eq!(s.statements, ["DROP TABLE public.b"]);
        assert_eq!(s.warnings.len(), 1);
    }

    #[test]
    fn a_key_added_between_existing_tables_is_an_alter() {
        let current = [
            table(
                "a",
                vec![column("id", "integer"), column("b_id", "integer")],
                &[],
                "public",
            ),
            table("b", vec![column("id", "integer")], &[], "public"),
        ];
        let mut design = current[0].clone();
        design.foreign_keys = table("a", vec![], &["b"], "public").foreign_keys;
        let s = edited_script(Dialect::Postgres, "public", &current, &[design], &[]).unwrap();
        assert_eq!(s.statements.len(), 1, "{:?}", s.statements);
        assert!(s.statements[0].starts_with("ALTER TABLE public.a ADD CONSTRAINT a_b"));
        assert!(s.statements[0].contains("REFERENCES public.b"));
    }

    #[test]
    fn a_column_renamed_on_the_diagram_is_renamed_not_dropped() {
        let current = [table("a", vec![column("mail", "text")], &[], "public")];
        let mut design = current[0].clone();
        design.columns[0].name = "email".to_owned();
        let s = edited_script(Dialect::Postgres, "public", &current, &[design], &[]).unwrap();
        assert_eq!(
            s.statements,
            ["ALTER TABLE public.a RENAME COLUMN mail TO email"],
            "{:?}",
            s.statements
        );
    }

    #[test]
    fn a_dropped_table_takes_the_keys_of_unedited_tables_with_it() {
        let current = [
            table(
                "orders",
                vec![column("id", "integer"), column("customers_id", "integer")],
                &["customers"],
                "public",
            ),
            table("customers", vec![column("id", "integer")], &[], "public"),
        ];
        let s = edited_script(
            Dialect::Postgres,
            "public",
            &current,
            &[],
            &["customers".to_owned()],
        )
        .unwrap();
        assert_eq!(s.statements.len(), 2, "{:?}", s.statements);
        assert!(s.statements[0].starts_with("ALTER TABLE public.orders DROP CONSTRAINT"));
        assert_eq!(s.statements[1], "DROP TABLE public.customers");
    }

    #[test]
    fn no_edits_need_nothing() {
        let current = [table("a", vec![column("id", "integer")], &[], "public")];
        let s = edited_script(Dialect::Sqlite, "main", &current, &[], &[]).unwrap();
        assert_eq!(s.statements, Vec::<String>::new());
    }

    #[test]
    fn user_types_are_taken_from_the_target_schema_and_created_there() {
        let mood = |schema: &str| crate::UserType {
            schema: schema.to_owned(),
            name: "mood".to_owned(),
            kind: "TYPE".to_owned(),
            body: "AS ENUM ('sad', 'ok')".to_owned(),
        };
        let mut e = table(
            "e",
            vec![ColumnDesign {
                default: Some("'ok'::src.mood".to_owned()),
                ..column("m", "src.mood")
            }],
            &[],
            "src",
        );
        e.columns.push(column("ms", "src.mood[]"));
        e.columns.push(column("moody", "src.moody"));
        e.types.push(mood("src"));
        let s = compare(
            Dialect::Postgres,
            "dst",
            "src",
            std::slice::from_ref(&e),
            &[],
        )
        .unwrap();
        assert_eq!(
            s.statements[0],
            "DO $platypus$ BEGIN CREATE TYPE dst.mood AS ENUM ('sad', 'ok'); \
             EXCEPTION WHEN duplicate_object THEN NULL; END $platypus$"
        );
        assert!(
            s.statements[1].contains("m dst.mood DEFAULT 'ok'::dst.mood"),
            "{s:?}"
        );
        assert!(s.statements[1].contains("ms dst.mood[]"), "{s:?}");
        assert!(s.statements[1].contains("moody src.moody"), "{s:?}");

        // Once there, the type and the table are the same.
        let mut there = e.clone();
        for c in &mut there.columns {
            if c.name != "moody" {
                c.data_type = c.data_type.replace("src.mood", "dst.mood");
            }
            c.default = c.default.as_ref().map(|d| d.replace("src.", "dst."));
        }
        there.types = vec![mood("dst")];
        let again = compare(
            Dialect::Postgres,
            "dst",
            "src",
            std::slice::from_ref(&e),
            &[there.clone()],
        );
        assert_eq!(again.unwrap().statements, Vec::<String>::new());
        there.types[0].body = "AS ENUM ('sad')".to_owned();
        let differs = compare(Dialect::Postgres, "dst", "src", &[e], &[there]).unwrap();
        assert_eq!(
            differs.warnings,
            ["The type dst.mood differs from the one compared with: change it by hand."]
        );
    }

    #[test]
    fn history_tables_come_and_go_with_their_system_versioned_table() {
        let mut tmp = table("tmp", vec![column("id", "int")], &[], "s");
        tmp.system_time = Some(crate::SystemTime {
            start: "vf".to_owned(),
            end: "vt".to_owned(),
            history: Some("tmp_hist".to_owned()),
        });
        let hist = table("tmp_hist", vec![column("id", "int")], &[], "s");
        let wanted = [tmp.clone(), hist.clone()];
        let s = compare(Dialect::SqlServer, "d", "s", &wanted, &[]).unwrap();
        assert_eq!(s.statements.len(), 1, "{s:?}");
        assert!(
            s.statements[0].ends_with("WITH (SYSTEM_VERSIONING = ON (HISTORY_TABLE = d.tmp_hist))"),
            "{s:?}"
        );
        let same = compare(Dialect::SqlServer, "d", "s", &wanted, &wanted).unwrap();
        assert_eq!(same.statements, Vec::<String>::new());
        let dropped = compare(Dialect::SqlServer, "d", "s", &[], &wanted).unwrap();
        assert_eq!(
            dropped.statements,
            [
                "ALTER TABLE d.tmp SET (SYSTEM_VERSIONING = OFF)",
                "DROP TABLE d.tmp",
                "DROP TABLE d.tmp_hist",
            ]
        );
        assert_eq!(dropped.warnings.len(), 2, "{dropped:?}");
    }

    #[test]
    fn names_are_replaced_whole() {
        assert_eq!(
            replace_name("a.mood, a.moody, xa.mood", "a.mood", "b.mood"),
            "b.mood, a.moody, xa.mood"
        );
        assert_eq!(
            replace_name("'ok'::mood", "::mood", "::b.mood"),
            "'ok'::b.mood"
        );
    }
}
