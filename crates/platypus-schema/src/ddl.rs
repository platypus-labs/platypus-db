//! The DDL from one [`TableDesign`] to another. Parts that changed are dropped and added
//! again, except columns, which are renamed and altered in place. Where the database cannot
//! alter a table that way (SQLite and DuckDB, beyond renaming and adding columns) the table is
//! created again and its rows copied.

use std::collections::HashMap;
use std::fmt::Write;

use platypus_core::ObjectKind;
use platypus_sql::{Dialect, ObjectName};

use crate::{
    ColumnDesign, ConstraintDesign, ConstraintType, ForeignKeyDesign, IndexDesign, Script,
    TableDesign,
};

/// The script that turns `original` into `design` in `schema`, or creates `design` when
/// `original` is `None`. `Err` says what in the design cannot be written.
pub fn script(
    dialect: Dialect,
    schema: &str,
    original: Option<&TableDesign>,
    design: &TableDesign,
) -> Result<Script, String> {
    validate(design)?;
    let mut script = Script {
        transactional: dialect != Dialect::MySql,
        ..Script::default()
    };
    let ddl = Ddl { d: dialect, schema };
    match original {
        None => {
            script
                .statements
                .push(ddl.create_table(design, &design.name));
            ddl.indexes_and_comments(design, &design.name, &mut script);
        }
        Some(old) => {
            let diff = Diff::new(old, design);
            if diff.needs_rebuild(dialect) {
                ddl.rebuild(old, design, &diff, &mut script);
            } else {
                ddl.alter(old, design, &diff, &mut script);
            }
        }
    }
    if dialect == Dialect::MySql && script.statements.len() > 1 {
        script.warnings.push(
            "MySQL commits each statement by itself: if one fails, the ones before it stay applied."
                .to_owned(),
        );
    }
    Ok(script)
}

fn validate(design: &TableDesign) -> Result<(), String> {
    if design.name.trim().is_empty() {
        return Err("The table needs a name.".to_owned());
    }
    if design.columns.is_empty() {
        return Err("The table needs at least one column.".to_owned());
    }
    for (i, c) in design.columns.iter().enumerate() {
        if c.name.trim().is_empty() {
            return Err(format!("Column {} has no name.", i + 1));
        }
        if c.data_type.trim().is_empty() {
            return Err(format!("Column {} has no type.", c.name));
        }
        if design.columns[..i].iter().any(|o| o.name == c.name) {
            return Err(format!("Two columns are named {}.", c.name));
        }
    }
    for i in &design.indexes {
        if i.name.trim().is_empty() || i.columns.is_empty() {
            return Err("Every index needs a name and at least one column.".to_owned());
        }
    }
    for f in &design.foreign_keys {
        if f.columns.is_empty() || f.ref_table.trim().is_empty() {
            return Err(
                "Every foreign key needs its columns and the table it references.".to_owned(),
            );
        }
        if !f.ref_columns.is_empty() && f.ref_columns.len() != f.columns.len() {
            return Err(format!(
                "Foreign key {}: as many referenced columns as columns are needed.",
                f.name
            ));
        }
    }
    for c in &design.constraints {
        match c.kind {
            ConstraintType::Unique if c.columns.is_empty() => {
                return Err("Every UNIQUE constraint needs at least one column.".to_owned());
            }
            ConstraintType::Check if c.check.trim().is_empty() => {
                return Err("Every CHECK constraint needs a condition.".to_owned());
            }
            _ => {}
        }
    }
    Ok(())
}

/// A trimmed, non-empty text.
fn text(value: Option<&String>) -> Option<&str> {
    value.map(|s| s.trim()).filter(|s| !s.is_empty())
}

/// The parts `old` and `new` have in common are found by the name the part had in the
/// database (`original`); a part that changed is in both `dropped` and `added`.
struct Parts<'a, T> {
    dropped: Vec<&'a T>,
    added: Vec<&'a T>,
}

impl<'a, T> Parts<'a, T> {
    fn new(
        old: &'a [T],
        new: &'a [T],
        key: impl Fn(&T) -> (Option<&str>, &str),
        same: impl Fn(&T, &T) -> bool,
    ) -> Self {
        let id = |t: &T| {
            let (original, name) = key(t);
            original.unwrap_or(name).to_owned()
        };
        let matching = |n: &T, o: &T| key(n).0 == Some(id(o).as_str());
        Self {
            dropped: old
                .iter()
                .filter(|o| !new.iter().any(|n| matching(n, o) && same(o, n)))
                .collect(),
            added: new
                .iter()
                .filter(|n| !old.iter().any(|o| matching(n, o) && same(o, n)))
                .collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.dropped.is_empty() && self.added.is_empty()
    }
}

struct Diff<'a> {
    dropped_columns: Vec<&'a ColumnDesign>,
    /// Columns kept, as they were and as they are now.
    kept_columns: Vec<(&'a ColumnDesign, &'a ColumnDesign)>,
    added_columns: Vec<&'a ColumnDesign>,
    primary_key_changed: bool,
    indexes: Parts<'a, IndexDesign>,
    foreign_keys: Parts<'a, ForeignKeyDesign>,
    constraints: Parts<'a, ConstraintDesign>,
}

impl<'a> Diff<'a> {
    fn new(old: &'a TableDesign, new: &'a TableDesign) -> Self {
        let old_column = |n: &ColumnDesign| {
            n.original
                .as_deref()
                .and_then(|o| old.columns.iter().find(|c| c.name == o))
        };
        let kept_columns: Vec<_> = new
            .columns
            .iter()
            .filter_map(|n| old_column(n).map(|o| (o, n)))
            .collect();
        // New names of renamed columns, by their name in the database.
        let renamed: HashMap<&str, &str> = kept_columns
            .iter()
            .filter(|(o, n)| o.name != n.name)
            .map(|(o, n)| (o.name.as_str(), n.name.as_str()))
            .collect();
        let map = |columns: &[String]| -> Vec<String> {
            columns
                .iter()
                .map(|c| {
                    renamed
                        .get(c.as_str())
                        .map_or_else(|| c.clone(), |n| (*n).to_owned())
                })
                .collect()
        };
        let mut old_key: Vec<String> = map(&primary_key(old));
        let mut new_key = primary_key(new);
        old_key.sort();
        new_key.sort();
        Self {
            dropped_columns: old
                .columns
                .iter()
                .filter(|o| !kept_columns.iter().any(|(k, _)| k.name == o.name))
                .collect(),
            added_columns: new
                .columns
                .iter()
                .filter(|n| old_column(n).is_none())
                .collect(),
            primary_key_changed: old_key != new_key,
            indexes: Parts::new(
                &old.indexes,
                &new.indexes,
                |i| (i.original.as_deref(), &i.name),
                |o, n| o.name == n.name && o.unique == n.unique && map(&o.columns) == n.columns,
            ),
            foreign_keys: Parts::new(
                &old.foreign_keys,
                &new.foreign_keys,
                |f| (f.original.as_deref(), &f.name),
                |o, n| {
                    o.name == n.name
                        && map(&o.columns) == n.columns
                        && o.ref_schema == n.ref_schema
                        && o.ref_table == n.ref_table
                        && o.ref_columns == n.ref_columns
                        && o.on_delete == n.on_delete
                        && o.on_update == n.on_update
                },
            ),
            constraints: Parts::new(
                &old.constraints,
                &new.constraints,
                |c| (c.original.as_deref(), &c.name),
                |o, n| {
                    o.name == n.name
                        && o.kind == n.kind
                        && map(&o.columns) == n.columns
                        && o.check.trim() == n.check.trim()
                },
            ),
            kept_columns,
        }
    }

    /// Whether the database cannot make these changes with `ALTER TABLE`.
    /// SQLite renames and adds plain columns; anything else means a new table. DuckDB
    /// alters little more, and not a column with a CHECK or an index on it.
    fn needs_rebuild(&self, dialect: Dialect) -> bool {
        matches!(dialect, Dialect::Sqlite | Dialect::DuckDb)
            && (self.primary_key_changed
                || !self.foreign_keys.is_empty()
                || !self.constraints.is_empty()
                || !self.dropped_columns.is_empty()
                || self
                    .kept_columns
                    .iter()
                    .any(|(o, n)| definition_changed(o, n))
                || self.added_columns.iter().any(|c| {
                    !c.nullable
                        || c.primary_key
                        || text(c.default.as_ref()).is_some_and(|d| !constant(d))
                }))
    }
}

/// Whether `default` is a literal, the only kind of default SQLite's `ADD COLUMN` takes.
fn constant(default: &str) -> bool {
    let d = default.trim();
    d.parse::<f64>().is_ok()
        || (d.len() >= 2 && d.starts_with('\'') && d.ends_with('\''))
        || ["NULL", "TRUE", "FALSE"]
            .iter()
            .any(|k| d.eq_ignore_ascii_case(k))
}

fn primary_key(design: &TableDesign) -> Vec<String> {
    design
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.clone())
        .collect()
}

fn type_changed(o: &ColumnDesign, n: &ColumnDesign) -> bool {
    o.data_type.trim() != n.data_type.trim()
}

fn default_changed(o: &ColumnDesign, n: &ColumnDesign) -> bool {
    text(o.default.as_ref()) != text(n.default.as_ref())
}

fn comment_changed(o: &ColumnDesign, n: &ColumnDesign) -> bool {
    text(o.comment.as_ref()) != text(n.comment.as_ref())
}

/// Whether anything but the column's name and comment changed.
fn definition_changed(o: &ColumnDesign, n: &ColumnDesign) -> bool {
    type_changed(o, n) || o.nullable != n.nullable || default_changed(o, n) || o.extra != n.extra
}

struct Ddl<'a> {
    d: Dialect,
    schema: &'a str,
}

impl Ddl<'_> {
    fn q(&self, name: &str) -> String {
        self.d.quote_ident(name).into_owned()
    }

    fn list(&self, names: &[String]) -> String {
        names
            .iter()
            .map(|n| self.q(n))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn table(&self, name: &str) -> String {
        self.d.qualified(Some(self.schema), name)
    }

    fn rename_table(&self, from: &str, to: &str) -> String {
        let o = ObjectName {
            schema: Some(self.schema),
            name: from,
            parent: None,
        };
        // Every dialect renames tables.
        let sql = self.d.rename(ObjectKind::Table, &o, to).unwrap_or_default();
        sql.trim_end_matches(';').to_owned()
    }

    fn column(&self, c: &ColumnDesign) -> String {
        let mut def = format!("{} {}", self.q(&c.name), c.data_type.trim());
        let extra = text(c.extra.as_ref());
        if self.d == Dialect::Sqlite && extra == Some("AUTOINCREMENT") {
            def.push_str(" PRIMARY KEY AUTOINCREMENT");
        }
        if !c.nullable {
            def.push_str(" NOT NULL");
        } else if self.d == Dialect::SqlServer {
            def.push_str(" NULL");
        }
        if let Some(default) = text(c.default.as_ref()) {
            def.push_str(" DEFAULT ");
            def.push_str(default);
        }
        if self.d == Dialect::MySql {
            if let Some(extra) = extra {
                def.push(' ');
                def.push_str(extra);
            }
            if let Some(comment) = text(c.comment.as_ref()) {
                def.push_str(" COMMENT ");
                def.push_str(&self.d.quote_literal(comment));
            }
        }
        def
    }

    fn named(&self, name: &str) -> String {
        if name.trim().is_empty() {
            String::new()
        } else {
            format!("CONSTRAINT {} ", self.q(name))
        }
    }

    fn constraint(&self, c: &ConstraintDesign) -> String {
        match c.kind {
            ConstraintType::Unique => {
                format!("{}UNIQUE ({})", self.named(&c.name), self.list(&c.columns))
            }
            ConstraintType::Check => format!("{}CHECK ({})", self.named(&c.name), c.check.trim()),
        }
    }

    fn foreign_key(&self, f: &ForeignKeyDesign) -> String {
        // SQLite references tables of the same schema only, by bare name.
        let target = if self.d == Dialect::Sqlite {
            self.q(&f.ref_table)
        } else {
            let schema = if f.ref_schema.is_empty() {
                self.schema
            } else {
                &f.ref_schema
            };
            self.d.qualified(Some(schema), &f.ref_table)
        };
        let mut sql = format!(
            "{}FOREIGN KEY ({}) REFERENCES {target}",
            self.named(&f.name),
            self.list(&f.columns)
        );
        if !f.ref_columns.is_empty() {
            let _ = write!(sql, " ({})", self.list(&f.ref_columns));
        }
        for (event, action) in [("DELETE", &f.on_delete), ("UPDATE", &f.on_update)] {
            if let Some(action) = text(action.as_ref()) {
                let _ = write!(sql, " ON {event} {action}");
            }
        }
        sql
    }

    fn create_table(&self, design: &TableDesign, name: &str) -> String {
        let mut parts: Vec<String> = design.columns.iter().map(|c| self.column(c)).collect();
        let key = primary_key(design);
        let inline_key = self.d == Dialect::Sqlite
            && design
                .columns
                .iter()
                .any(|c| c.primary_key && text(c.extra.as_ref()) == Some("AUTOINCREMENT"));
        if !key.is_empty() && !inline_key {
            parts.push(format!("PRIMARY KEY ({})", self.list(&key)));
        }
        parts.extend(design.constraints.iter().map(|c| self.constraint(c)));
        parts.extend(design.foreign_keys.iter().map(|f| self.foreign_key(f)));
        format!(
            "CREATE TABLE {} (\n  {}\n)",
            self.table(name),
            parts.join(",\n  ")
        )
    }

    fn create_index(&self, table: &str, i: &IndexDesign) -> String {
        let unique = if i.unique { "UNIQUE " } else { "" };
        // SQLite qualifies the index, not its table.
        let (index, on) = if self.d == Dialect::Sqlite {
            (self.table(&i.name), self.q(table))
        } else {
            (self.q(&i.name), self.table(table))
        };
        format!(
            "CREATE {unique}INDEX {index} ON {on} ({})",
            self.list(&i.columns)
        )
    }

    fn drop_index(&self, table: &str, name: &str) -> String {
        match self.d {
            Dialect::MySql | Dialect::SqlServer => {
                format!("DROP INDEX {} ON {}", self.q(name), self.table(table))
            }
            _ => format!("DROP INDEX {}", self.table(name)),
        }
    }

    /// The indexes of `design` on `table` and the comments of its columns.
    fn indexes_and_comments(&self, design: &TableDesign, table: &str, script: &mut Script) {
        for i in &design.indexes {
            script.statements.push(self.create_index(table, i));
        }
        let commented: Vec<&ColumnDesign> = design
            .columns
            .iter()
            .filter(|c| text(c.comment.as_ref()).is_some())
            .collect();
        self.comments(table, &commented, script);
    }

    /// Sets the comments of `columns`; MySQL has them in the column definitions already.
    fn comments(&self, table: &str, columns: &[&ColumnDesign], script: &mut Script) {
        if columns.is_empty() {
            return;
        }
        match self.d {
            Dialect::Postgres | Dialect::DuckDb => {
                for c in columns {
                    let comment = text(c.comment.as_ref())
                        .map_or_else(|| "NULL".to_owned(), |t| self.d.quote_literal(t));
                    script.statements.push(format!(
                        "COMMENT ON COLUMN {}.{} IS {comment}",
                        self.table(table),
                        self.q(&c.name)
                    ));
                }
            }
            Dialect::MySql => {}
            // shortcut: SQL Server keeps comments as extended properties, not written here;
            // add sp_addextendedproperty when someone asks.
            Dialect::SqlServer => script
                .warnings
                .push("Column comments are not changed on SQL Server.".to_owned()),
            Dialect::Sqlite => script
                .warnings
                .push("SQLite has no column comments: they are not saved.".to_owned()),
        }
    }

    /// Drops the default constraint of `column`, whatever its name: SQL Server cannot drop a
    /// column or change its default while one is there. One statement, so that it runs as
    /// such in the editor too.
    fn drop_mssql_default(&self, table: &str, column: &str) -> String {
        let d = self.d;
        let object = d.quote_literal(&self.table(table));
        let drop = d.quote_literal(&format!(
            "ALTER TABLE {} DROP CONSTRAINT ",
            self.table(table)
        ));
        let inner = format!(
            "DECLARE @n sysname = (SELECT name FROM sys.default_constraints \
             WHERE parent_object_id = OBJECT_ID({object}) \
             AND parent_column_id = COLUMNPROPERTY(OBJECT_ID({object}), {}, 'ColumnId')); \
             DECLARE @s nvarchar(max) = {drop} + QUOTENAME(@n); IF @n IS NOT NULL EXEC(@s)",
            d.quote_literal(column)
        );
        format!("EXEC({})", d.quote_literal(&inner))
    }

    fn alter(&self, old: &TableDesign, new: &TableDesign, diff: &Diff<'_>, script: &mut Script) {
        let out = &mut script.statements;
        let mut name = old.name.as_str();
        if new.name != old.name {
            out.push(self.rename_table(name, &new.name));
            name = &new.name;
        }
        let t = self.table(name);
        for f in &diff.foreign_keys.dropped {
            let original = self.q(f.original.as_deref().unwrap_or(&f.name));
            out.push(match self.d {
                Dialect::MySql => format!("ALTER TABLE {t} DROP FOREIGN KEY {original}"),
                _ => format!("ALTER TABLE {t} DROP CONSTRAINT {original}"),
            });
        }
        for c in &diff.constraints.dropped {
            let original = c.original.as_deref().unwrap_or(&c.name);
            out.push(match (self.d, c.kind) {
                (Dialect::MySql, ConstraintType::Unique) => {
                    format!("ALTER TABLE {t} DROP INDEX {}", self.q(original))
                }
                _ => format!("ALTER TABLE {t} DROP CONSTRAINT {}", self.q(original)),
            });
        }
        for i in &diff.indexes.dropped {
            out.push(self.drop_index(name, i.original.as_deref().unwrap_or(&i.name)));
        }
        if diff.primary_key_changed && !primary_key(old).is_empty() {
            match (self.d, &old.primary_key_name) {
                (Dialect::MySql, _) => out.push(format!("ALTER TABLE {t} DROP PRIMARY KEY")),
                (_, Some(key)) => {
                    out.push(format!("ALTER TABLE {t} DROP CONSTRAINT {}", self.q(key)));
                }
                (_, None) => script
                    .warnings
                    .push("The primary key has no known name: drop it by hand.".to_owned()),
            }
        }
        let out = &mut script.statements;
        for c in &diff.dropped_columns {
            if self.d == Dialect::SqlServer && c.default.is_some() {
                out.push(self.drop_mssql_default(name, &c.name));
            }
            out.push(format!("ALTER TABLE {t} DROP COLUMN {}", self.q(&c.name)));
        }
        for (o, n) in &diff.kept_columns {
            self.alter_column(name, o, n, out);
        }
        for c in &diff.added_columns {
            let add = if self.d == Dialect::SqlServer {
                "ADD"
            } else {
                "ADD COLUMN"
            };
            // Without WITH VALUES SQL Server leaves the rows there NULL, unlike the others.
            let values = if self.d == Dialect::SqlServer && c.nullable && c.default.is_some() {
                " WITH VALUES"
            } else {
                ""
            };
            out.push(format!("ALTER TABLE {t} {add} {}{values}", self.column(c)));
        }
        let key = primary_key(new);
        if diff.primary_key_changed && !key.is_empty() {
            out.push(format!(
                "ALTER TABLE {t} ADD PRIMARY KEY ({})",
                self.list(&key)
            ));
        }
        for c in &diff.constraints.added {
            out.push(format!("ALTER TABLE {t} ADD {}", self.constraint(c)));
        }
        for f in &diff.foreign_keys.added {
            out.push(format!("ALTER TABLE {t} ADD {}", self.foreign_key(f)));
        }
        for i in &diff.indexes.added {
            out.push(self.create_index(name, i));
        }
        let commented: Vec<&ColumnDesign> = diff
            .kept_columns
            .iter()
            .filter(|(o, n)| comment_changed(o, n))
            .map(|(_, n)| *n)
            .chain(
                diff.added_columns
                    .iter()
                    .copied()
                    .filter(|c| text(c.comment.as_ref()).is_some()),
            )
            .collect();
        self.comments(name, &commented, script);
    }

    /// Renames and redefines a kept column, on `table` (its new name).
    fn alter_column(&self, table: &str, o: &ColumnDesign, n: &ColumnDesign, out: &mut Vec<String>) {
        let t = self.table(table);
        if self.d == Dialect::MySql {
            if o.name != n.name || definition_changed(o, n) || comment_changed(o, n) {
                out.push(format!(
                    "ALTER TABLE {t} CHANGE COLUMN {} {}",
                    self.q(&o.name),
                    self.column(n)
                ));
            }
            return;
        }
        if o.name != n.name {
            out.push(if self.d == Dialect::SqlServer {
                format!(
                    "EXEC sp_rename {}, {}, 'COLUMN'",
                    self.d.quote_literal(&format!("{t}.{}", self.q(&o.name))),
                    self.d.quote_literal(&n.name)
                )
            } else {
                format!(
                    "ALTER TABLE {t} RENAME COLUMN {} TO {}",
                    self.q(&o.name),
                    self.q(&n.name)
                )
            });
        }
        let c = self.q(&n.name);
        let ty = n.data_type.trim();
        if self.d == Dialect::SqlServer {
            if type_changed(o, n) || o.nullable != n.nullable {
                let null = if n.nullable { "NULL" } else { "NOT NULL" };
                out.push(format!("ALTER TABLE {t} ALTER COLUMN {c} {ty} {null}"));
            }
            if default_changed(o, n) {
                if o.default.is_some() {
                    out.push(self.drop_mssql_default(table, &n.name));
                }
                if let Some(default) = text(n.default.as_ref()) {
                    out.push(format!("ALTER TABLE {t} ADD DEFAULT {default} FOR {c}"));
                }
            }
            return;
        }
        // PostgreSQL; SQLite and DuckDB rebuild instead.
        if type_changed(o, n) {
            out.push(format!(
                "ALTER TABLE {t} ALTER COLUMN {c} TYPE {ty} USING {c}::{ty}"
            ));
        }
        if o.nullable != n.nullable {
            let set = if n.nullable { "DROP" } else { "SET" };
            out.push(format!("ALTER TABLE {t} ALTER COLUMN {c} {set} NOT NULL"));
        }
        if default_changed(o, n) {
            out.push(match text(n.default.as_ref()) {
                Some(default) => format!("ALTER TABLE {t} ALTER COLUMN {c} SET DEFAULT {default}"),
                None => format!("ALTER TABLE {t} ALTER COLUMN {c} DROP DEFAULT"),
            });
        }
    }

    /// Copies the rows aside, drops the old table, creates the new one and copies the rows of
    /// the kept columns back. No table is renamed: DuckDB's foreign keys would keep the
    /// temporary name.
    fn rebuild(&self, old: &TableDesign, new: &TableDesign, diff: &Diff<'_>, script: &mut Script) {
        let rows = self.table(&format!("_platypus_{}", old.name));
        if self.d == Dialect::Sqlite {
            script.before.push("PRAGMA foreign_keys = OFF".to_owned());
            script.after.push("PRAGMA foreign_keys = ON".to_owned());
        }
        let out = &mut script.statements;
        let table = self.table(&old.name);
        let copy = !diff.kept_columns.is_empty();
        if copy {
            out.push(format!("CREATE TABLE {rows} AS SELECT * FROM {table}"));
        }
        out.push(format!("DROP TABLE {table}"));
        out.push(self.create_table(new, &new.name));
        if copy {
            let (from, to): (Vec<String>, Vec<String>) = diff
                .kept_columns
                .iter()
                .map(|(o, n)| (o.name.clone(), n.name.clone()))
                .unzip();
            out.push(format!(
                "INSERT INTO {} ({}) SELECT {} FROM {rows}",
                self.table(&new.name),
                self.list(&to),
                self.list(&from),
            ));
            out.push(format!("DROP TABLE {rows}"));
        }
        let database = if self.d == Dialect::Sqlite {
            "SQLite"
        } else {
            "DuckDB"
        };
        let mut warning = format!(
            "{database} cannot make these changes in place: the table is created again and its rows copied."
        );
        if self.d == Dialect::Sqlite {
            warning.push_str(" Its triggers are dropped with it.");
        }
        script.warnings.push(warning);
        // The drivers list an expression index's key parts as `(expression)`, not its SQL.
        let (indexes, expressions): (Vec<IndexDesign>, Vec<IndexDesign>) = new
            .indexes
            .iter()
            .cloned()
            .partition(|i| !i.columns.iter().any(|c| c == "(expression)"));
        for i in &expressions {
            script.warnings.push(format!(
                "Index {} is on an expression: create it again by hand.",
                i.name
            ));
        }
        let new = TableDesign {
            indexes,
            ..new.clone()
        };
        self.indexes_and_comments(&new, &new.name, script);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, data_type: &str) -> ColumnDesign {
        ColumnDesign {
            original: Some(name.to_owned()),
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable: true,
            ..ColumnDesign::default()
        }
    }

    fn orders() -> TableDesign {
        TableDesign {
            name: "orders".to_owned(),
            columns: vec![
                ColumnDesign {
                    primary_key: true,
                    nullable: false,
                    ..column("id", "integer")
                },
                column("total", "numeric(10,2)"),
                column("note", "text"),
            ],
            primary_key_name: Some("orders_pkey".to_owned()),
            indexes: vec![IndexDesign {
                original: Some("orders_total".to_owned()),
                name: "orders_total".to_owned(),
                columns: vec!["total".to_owned()],
                unique: false,
            }],
            ..TableDesign::default()
        }
    }

    #[test]
    fn creates_tables_with_keys_constraints_and_indexes() {
        let mut design = orders();
        design.constraints.push(ConstraintDesign {
            name: "positive".to_owned(),
            kind: ConstraintType::Check,
            check: "total >= 0".to_owned(),
            ..ConstraintDesign::default()
        });
        design.foreign_keys.push(ForeignKeyDesign {
            name: "orders_customer".to_owned(),
            columns: vec!["note".to_owned()],
            ref_schema: "crm".to_owned(),
            ref_table: "customers".to_owned(),
            ref_columns: vec!["code".to_owned()],
            on_delete: Some("CASCADE".to_owned()),
            ..ForeignKeyDesign::default()
        });
        design.columns[2].comment = Some("free text".to_owned());
        let s = script(Dialect::Postgres, "public", None, &design).unwrap();
        assert_eq!(
            s.statements,
            [
                "CREATE TABLE public.orders (\n  id integer NOT NULL,\n  total numeric(10,2),\n  note text,\n  \
                 PRIMARY KEY (id),\n  CONSTRAINT positive CHECK (total >= 0),\n  \
                 CONSTRAINT orders_customer FOREIGN KEY (note) REFERENCES crm.customers (code) ON DELETE CASCADE\n)",
                "CREATE INDEX orders_total ON public.orders (total)",
                "COMMENT ON COLUMN public.orders.note IS 'free text'",
            ]
        );
        assert!(s.transactional);
        let mysql = script(Dialect::MySql, "shop", None, &design).unwrap();
        assert!(
            mysql.statements[0].contains("note text COMMENT 'free text'"),
            "{mysql:?}"
        );
        assert_eq!(mysql.statements.len(), 2);
    }

    #[test]
    fn alters_columns_in_place() {
        let old = orders();
        let mut new = old.clone();
        new.columns[1].name = "amount".to_owned();
        new.columns[1].data_type = "numeric(12,2)".to_owned();
        new.columns[1].nullable = false;
        new.columns[1].default = Some("0".to_owned());
        new.columns.remove(2);
        new.columns.push(ColumnDesign {
            original: None,
            ..column("status", "text")
        });
        new.indexes[0].columns = vec!["amount".to_owned()];
        let pg = script(Dialect::Postgres, "public", Some(&old), &new).unwrap();
        assert_eq!(
            pg.statements,
            [
                "ALTER TABLE public.orders DROP COLUMN note",
                "ALTER TABLE public.orders RENAME COLUMN total TO amount",
                "ALTER TABLE public.orders ALTER COLUMN amount TYPE numeric(12,2) USING amount::numeric(12,2)",
                "ALTER TABLE public.orders ALTER COLUMN amount SET NOT NULL",
                "ALTER TABLE public.orders ALTER COLUMN amount SET DEFAULT 0",
                "ALTER TABLE public.orders ADD COLUMN status text",
            ],
            "the renamed index column is the same index"
        );
        let mysql = script(Dialect::MySql, "shop", Some(&old), &new).unwrap();
        assert_eq!(
            mysql.statements[1],
            "ALTER TABLE shop.orders CHANGE COLUMN total amount numeric(12,2) NOT NULL DEFAULT 0"
        );
        assert!(!mysql.transactional);
        assert_eq!(mysql.warnings.len(), 1);
        let server = script(Dialect::SqlServer, "dbo", Some(&old), &new).unwrap();
        assert_eq!(
            server.statements[1..4],
            [
                "EXEC sp_rename N'dbo.orders.total', N'amount', 'COLUMN'",
                "ALTER TABLE dbo.orders ALTER COLUMN amount numeric(12,2) NOT NULL",
                "ALTER TABLE dbo.orders ADD DEFAULT 0 FOR amount",
            ]
        );
        assert_eq!(
            server.statements[4],
            "ALTER TABLE dbo.orders ADD status text NULL"
        );
    }

    #[test]
    fn changed_parts_are_dropped_and_added_again() {
        let old = orders();
        let mut new = old.clone();
        new.indexes[0].unique = true;
        new.columns[1].primary_key = true;
        new.columns[1].nullable = false;
        let s = script(Dialect::Postgres, "public", Some(&old), &new).unwrap();
        assert_eq!(
            s.statements,
            [
                "DROP INDEX public.orders_total",
                "ALTER TABLE public.orders DROP CONSTRAINT orders_pkey",
                "ALTER TABLE public.orders ALTER COLUMN total SET NOT NULL",
                "ALTER TABLE public.orders ADD PRIMARY KEY (id, total)",
                "CREATE UNIQUE INDEX orders_total ON public.orders (total)",
            ]
        );
        let mysql = script(Dialect::MySql, "shop", Some(&old), &new).unwrap();
        assert_eq!(
            mysql.statements[0],
            "DROP INDEX orders_total ON shop.orders"
        );
        assert_eq!(
            mysql.statements[1],
            "ALTER TABLE shop.orders DROP PRIMARY KEY"
        );
    }

    #[test]
    fn unchanged_designs_need_nothing() {
        let old = orders();
        for d in [
            Dialect::Postgres,
            Dialect::MySql,
            Dialect::Sqlite,
            Dialect::DuckDb,
            Dialect::SqlServer,
        ] {
            assert_eq!(
                script(d, "s", Some(&old), &old).unwrap().statements,
                Vec::<String>::new()
            );
        }
    }

    #[test]
    fn sqlite_rebuilds_what_it_cannot_alter() {
        let mut old = orders();
        old.columns[0].extra = Some("AUTOINCREMENT".to_owned());
        let mut new = old.clone();
        new.columns[1].nullable = false;
        new.columns.remove(2);
        new.name = "purchases".to_owned();
        let s = script(Dialect::Sqlite, "main", Some(&old), &new).unwrap();
        assert_eq!(s.before, ["PRAGMA foreign_keys = OFF"]);
        assert_eq!(s.after, ["PRAGMA foreign_keys = ON"]);
        assert_eq!(
            s.statements,
            [
                "CREATE TABLE main._platypus_orders AS SELECT * FROM main.orders",
                "DROP TABLE main.orders",
                "CREATE TABLE main.purchases (\n  id integer PRIMARY KEY AUTOINCREMENT NOT NULL,\n  \
                 total numeric(10,2) NOT NULL\n)",
                "INSERT INTO main.purchases (id, total) SELECT id, total FROM main._platypus_orders",
                "DROP TABLE main._platypus_orders",
                "CREATE INDEX main.orders_total ON purchases (total)",
            ]
        );
        assert_eq!(s.warnings.len(), 1);

        // Renames and plain new columns stay in place.
        let mut new = old.clone();
        new.columns[2].name = "remark".to_owned();
        new.columns.push(ColumnDesign {
            original: None,
            ..column("status", "text")
        });
        let s = script(Dialect::Sqlite, "main", Some(&old), &new).unwrap();
        assert_eq!(
            s.statements,
            [
                "ALTER TABLE main.orders RENAME COLUMN note TO remark",
                "ALTER TABLE main.orders ADD COLUMN status text",
            ]
        );
        assert_eq!(s.before, Vec::<String>::new());
    }

    #[test]
    fn mssql_drops_default_constraints_by_their_generated_name() {
        let mut old = orders();
        old.columns[2].default = Some("('x')".to_owned());
        let mut new = old.clone();
        new.columns.remove(2);
        let s = script(Dialect::SqlServer, "dbo", Some(&old), &new).unwrap();
        assert!(
            s.statements[0].starts_with("EXEC(N'DECLARE @n sysname"),
            "{s:?}"
        );
        assert!(
            s.statements[0].contains("OBJECT_ID(N''dbo.orders'')"),
            "{s:?}"
        );
        assert_eq!(s.statements[1], "ALTER TABLE dbo.orders DROP COLUMN note");
    }

    #[test]
    fn foreign_keys_follow_renamed_columns_and_actions() {
        let mut old = orders();
        old.foreign_keys.push(ForeignKeyDesign {
            original: Some("fk".to_owned()),
            name: "fk".to_owned(),
            columns: vec!["note".to_owned()],
            ref_schema: "public".to_owned(),
            ref_table: "notes".to_owned(),
            ref_columns: vec!["id".to_owned()],
            ..ForeignKeyDesign::default()
        });
        let mut renamed = old.clone();
        renamed.columns[2].name = "remark".to_owned();
        renamed.foreign_keys[0].columns = vec!["remark".to_owned()];
        let s = script(Dialect::Postgres, "public", Some(&old), &renamed).unwrap();
        assert_eq!(
            s.statements,
            ["ALTER TABLE public.orders RENAME COLUMN note TO remark"]
        );

        let mut cascading = old.clone();
        cascading.foreign_keys[0].on_delete = Some("CASCADE".to_owned());
        let s = script(Dialect::MySql, "public", Some(&old), &cascading).unwrap();
        assert_eq!(
            s.statements,
            [
                "ALTER TABLE public.orders DROP FOREIGN KEY fk",
                "ALTER TABLE public.orders ADD CONSTRAINT fk FOREIGN KEY (note) REFERENCES public.notes (id) ON DELETE CASCADE",
            ]
        );
        let s = script(Dialect::DuckDb, "main", Some(&old), &cascading).unwrap();
        assert!(
            s.statements[0].starts_with("CREATE TABLE main._platypus_orders AS"),
            "{s:?}"
        );
    }

    #[test]
    fn review_cases_of_new_columns_and_rebuilds() {
        let old = orders();
        let added = |default: &str| {
            let mut new = old.clone();
            new.columns.push(ColumnDesign {
                original: None,
                default: Some(default.to_owned()),
                ..column("status", "text")
            });
            new
        };
        // SQLite adds a column with a literal default in place, not one with an expression.
        let s = script(Dialect::Sqlite, "main", Some(&old), &added("'new'")).unwrap();
        assert_eq!(
            s.statements,
            ["ALTER TABLE main.orders ADD COLUMN status text DEFAULT 'new'"]
        );
        let s = script(
            Dialect::Sqlite,
            "main",
            Some(&old),
            &added("CURRENT_TIMESTAMP"),
        )
        .unwrap();
        assert!(s.statements[0].contains(" AS SELECT "), "{s:?}");
        // SQL Server fills the rows already there.
        let s = script(Dialect::SqlServer, "dbo", Some(&old), &added("'new'")).unwrap();
        assert_eq!(
            s.statements,
            ["ALTER TABLE dbo.orders ADD status text NULL DEFAULT 'new' WITH VALUES"]
        );
        // A rebuild leaves out expression indexes, with a warning.
        let mut old = orders();
        old.indexes[0].columns = vec!["(expression)".to_owned()];
        let mut new = old.clone();
        new.columns.remove(2);
        let s = script(Dialect::Sqlite, "main", Some(&old), &new).unwrap();
        assert!(
            !s.statements.iter().any(|x| x.contains("expression")),
            "{s:?}"
        );
        assert_eq!(s.warnings.len(), 2, "{s:?}");
    }

    #[test]
    fn rejects_incomplete_designs() {
        let mut design = orders();
        design.columns.push(ColumnDesign {
            primary_key: false,
            ..design.columns[0].clone()
        });
        assert_eq!(
            script(Dialect::Postgres, "s", None, &design),
            Err("Two columns are named id.".to_owned())
        );
        let mut design = orders();
        design.columns[1].data_type = " ".to_owned();
        assert!(script(Dialect::Postgres, "s", None, &design).is_err());
        assert!(script(Dialect::Postgres, "s", None, &TableDesign::default()).is_err());
    }
}
