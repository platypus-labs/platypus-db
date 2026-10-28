//! The DDL from one [`TableDesign`] to another. Parts that changed are dropped and added
//! again, except columns, which are renamed and altered in place. Where the database cannot
//! alter a table that way (SQLite and DuckDB, beyond renaming and adding columns) the table is
//! created again and its rows copied.

use std::collections::HashMap;
use std::fmt::Write;

use platypus_core::ObjectKind;
use platypus_sql::{Dialect, Lexer, ObjectName, Token, TokenKind};

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
    let mut script = phased(dialect, schema, original, design, None)?;
    warn_one_by_one(dialect, &mut script);
    Ok(script)
}

/// Whether the database runs DDL in a transaction: MySQL and Oracle commit each DDL
/// statement, ClickHouse has no transactions.
pub(crate) fn transactional(dialect: Dialect) -> bool {
    !matches!(
        dialect,
        Dialect::MySql | Dialect::ClickHouse | Dialect::Oracle
    )
}

/// Warns, once, that `script` runs one statement at a time when it cannot run in one
/// transaction and has more than one statement.
pub(crate) fn warn_one_by_one(dialect: Dialect, script: &mut Script) {
    if script.transactional || script.statements.len() <= 1 {
        return;
    }
    let database = match dialect {
        Dialect::MySql => "MySQL",
        Dialect::Oracle => "Oracle",
        _ => "ClickHouse",
    };
    let warning = format!(
        "{database} runs each statement by itself: if one fails, the ones before it stay applied."
    );
    if !script.warnings.contains(&warning) {
        script.warnings.push(warning);
    }
}

/// The foreign keys an `ALTER TABLE` script drops and adds, kept apart by [`phased`]: a
/// schema comparison drops those of every table first and adds them last, once the
/// columns and tables they reference are there.
#[derive(Default)]
pub(crate) struct Keys {
    pub dropped: Vec<String>,
    pub added: Vec<String>,
}

/// Like [`script`], with the foreign keys an `ALTER TABLE` drops and adds in `keys`, when
/// given, instead of in the script (a table created again keeps them in its script).
pub(crate) fn phased(
    dialect: Dialect,
    schema: &str,
    original: Option<&TableDesign>,
    design: &TableDesign,
    keys: Option<&mut Keys>,
) -> Result<Script, String> {
    validate(dialect, design)?;
    let mut script = Script {
        transactional: transactional(dialect),
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
                ddl.alter(old, design, &diff, &mut script, keys);
                if text(old.comment.as_ref()) != text(design.comment.as_ref()) {
                    ddl.push_table_comment(
                        design,
                        text(old.comment.as_ref()).is_some(),
                        &mut script,
                    );
                }
            }
        }
    }
    Ok(script)
}

/// `ALTER TABLE .. ADD` of the foreign key `fk` of `schema.table`.
pub(crate) fn add_foreign_key(
    dialect: Dialect,
    schema: &str,
    table: &str,
    fk: &ForeignKeyDesign,
) -> String {
    let ddl = Ddl { d: dialect, schema };
    format!(
        "ALTER TABLE {} ADD {}",
        ddl.table(table),
        ddl.foreign_key(fk)
    )
}

/// `CREATE INDEX` of `index` on `schema.table`.
pub(crate) fn create_index(
    dialect: Dialect,
    schema: &str,
    table: &str,
    index: &IndexDesign,
) -> String {
    Ddl { d: dialect, schema }.create_index(table, index)
}

/// `DROP INDEX` of the index `name` on `schema.table`.
pub(crate) fn drop_index(dialect: Dialect, schema: &str, table: &str, name: &str) -> String {
    Ddl { d: dialect, schema }.drop_index(table, name)
}

fn validate(dialect: Dialect, design: &TableDesign) -> Result<(), String> {
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
        // SQLite's columns can be declared without a type: `CREATE TABLE t (x)`.
        if c.data_type.trim().is_empty() && dialect != Dialect::Sqlite {
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
                        && same_check(&o.check, &n.check)
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
                    text(c.generated.as_ref()).is_some()
                        || !c.nullable
                        || c.primary_key
                        || text(c.default.as_ref()).is_some_and(|d| !constant(d))
                }))
    }
}

/// Whether two CHECK conditions are the same, as the database prints them: PostgreSQL
/// prints a condition it was given in its own words another way (`'a'::character varying`
/// becomes `('a'::character varying)::text`), so casts and parentheses around a single
/// term are left out, and so are case and spaces outside quotes.
pub(crate) fn same_check(a: &str, b: &str) -> bool {
    a.trim() == b.trim() || check_key(a) == check_key(b)
}

/// The words a type name of more than one word goes on with: `character varying`,
/// `timestamp without time zone`, `double precision`.
const TYPE_WORDS: [&str; 6] = ["varying", "precision", "without", "with", "time", "zone"];

/// The tokens of a CHECK condition as [`same_check`] compares them.
fn check_key(text: &str) -> Vec<String> {
    let mut tokens = check_tokens(text);
    loop {
        let pairs = matching_parens(&tokens);
        let removable = pairs.iter().find(|&&(open, close)| {
            close == open + 2
                || (tokens[open + 1] == "(" && pairs.contains(&(open + 1, close - 1)))
                || (open == 0 && close == tokens.len() - 1)
        });
        match removable {
            Some(&(open, close)) => {
                tokens.remove(close);
                tokens.remove(open);
            }
            None => return tokens,
        }
    }
}

/// The pairs of matching parentheses of `tokens`, by position.
fn matching_parens(tokens: &[String]) -> Vec<(usize, usize)> {
    let mut open = Vec::new();
    let mut pairs = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        match t.as_str() {
            "(" => open.push(i),
            ")" => {
                if let Some(o) = open.pop() {
                    pairs.push((o, i));
                }
            }
            _ => {}
        }
    }
    pairs
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '$' | '#' | '.')
}

/// The tokens of `text` without its casts, words in lower case.
fn check_tokens(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        i += 1;
        let close = match c {
            '\'' | '"' | '`' => Some(c),
            // `[name]` in Transact-SQL; `ARRAY[..]` in PostgreSQL.
            '[' if tokens.last().is_none_or(|t| t != "array") => Some(']'),
            _ => None,
        };
        if let Some(close) = close {
            // A quoted text, its doubled quotes included.
            while i < chars.len() {
                if chars[i] == close {
                    if chars.get(i + 1) == Some(&close) && close != ']' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            tokens.push(chars[start..i].iter().collect());
        } else if is_word_char(c) {
            while i < chars.len() && is_word_char(chars[i]) {
                i += 1;
            }
            tokens.push(chars[start..i].iter().collect::<String>().to_lowercase());
        } else if c == ':' && chars.get(i) == Some(&':') {
            i = skip_cast_type(&chars, i + 1);
        } else if !c.is_whitespace() {
            tokens.push(c.to_string());
        }
    }
    tokens
}

/// Where the type of a cast that starts at `i` ends: its words, its `(..)` and its `[]`.
fn skip_cast_type(chars: &[char], mut i: usize) -> usize {
    let blank = |mut j: usize| {
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        j
    };
    // The word at `i` (after spaces): where it ends, and the word in lower case.
    let word_at = |i: usize| -> (usize, String) {
        let start = blank(i);
        let mut j = start;
        if chars.get(j) == Some(&'"') {
            j += 1;
            while j < chars.len() && chars[j] != '"' {
                j += 1;
            }
            let end = (j + 1).min(chars.len());
            return (end, chars[start..end].iter().collect());
        }
        while j < chars.len() && (chars[j].is_alphanumeric() || matches!(chars[j], '_' | '.')) {
            j += 1;
        }
        (j, chars[start..j].iter().collect::<String>().to_lowercase())
    };
    i = word_at(i).0;
    loop {
        let (end, word) = word_at(i);
        if !TYPE_WORDS.contains(&word.as_str()) {
            break;
        }
        i = end;
    }
    let past = |i: usize, open: char, close: char| -> Option<usize> {
        let j = blank(i);
        (chars.get(j) == Some(&open)).then(|| {
            chars[j..]
                .iter()
                .position(|&c| c == close)
                .map_or(chars.len(), |p| j + p + 1)
        })
    };
    if let Some(end) = past(i, '(', ')') {
        i = end;
    }
    while let Some(end) = past(i, '[', ']') {
        i = end;
    }
    i
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
    type_changed(o, n)
        || o.nullable != n.nullable
        || default_changed(o, n)
        || o.extra != n.extra
        || text(o.generated.as_ref()) != text(n.generated.as_ref())
        || text(o.collation.as_ref()) != text(n.collation.as_ref())
}

/// Whether a ClickHouse default (as its driver reports it) has its own keyword instead of
/// `DEFAULT`: `MATERIALIZED x`, `ALIAS x`, `EPHEMERAL x`.
fn clickhouse_default_kind(default: &str) -> bool {
    let word = default.split_whitespace().next().unwrap_or_default();
    ["MATERIALIZED", "ALIAS", "EPHEMERAL"]
        .iter()
        .any(|k| word.eq_ignore_ascii_case(k))
}

/// A ClickHouse type made `Nullable` or not, as the column's `nullable` says.
fn clickhouse_type(c: &ColumnDesign) -> String {
    let ty = c.data_type.trim();
    let inner = ty
        .strip_prefix("Nullable(")
        .and_then(|t| t.strip_suffix(')'));
    match (c.nullable, inner) {
        // `LowCardinality(Nullable(String))` is nullable already.
        (true, None) if !ty.contains("Nullable(") => format!("Nullable({ty})"),
        (false, Some(inner)) => inner.to_owned(),
        _ => ty.to_owned(),
    }
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
        // ClickHouse writes nullability in the type.
        let clickhouse = self.d == Dialect::ClickHouse;
        let ty = if clickhouse {
            clickhouse_type(c)
        } else {
            c.data_type.trim().to_owned()
        };
        let mut def = self.q(&c.name);
        if !ty.is_empty() {
            let _ = write!(def, " {ty}");
        }
        if let Some(collation) = text(c.collation.as_ref()) {
            let _ = write!(def, " COLLATE {collation}");
        }
        let extra = text(c.extra.as_ref());
        if self.d == Dialect::Sqlite && extra == Some("AUTOINCREMENT") {
            def.push_str(" PRIMARY KEY AUTOINCREMENT");
        }
        // Oracle's identity: `GENERATED BY DEFAULT AS IDENTITY`.
        if let (Dialect::Oracle, Some(extra)) = (self.d, extra) {
            def.push(' ');
            def.push_str(extra);
        }
        let generated = text(c.generated.as_ref());
        if let Some(clause) = generated {
            def.push(' ');
            def.push_str(clause);
        }
        let null = if !c.nullable && !clickhouse {
            " NOT NULL"
        } else if self.d == Dialect::SqlServer {
            " NULL"
        } else {
            ""
        };
        let default = match text(c.default.as_ref()) {
            // A generated column takes no default.
            Some(_) if generated.is_some() => String::new(),
            // ClickHouse's other kinds of default are their own keyword.
            Some(default) if clickhouse && clickhouse_default_kind(default) => {
                format!(" {default}")
            }
            Some(default) => format!(" DEFAULT {default}"),
            None => String::new(),
        };
        // Oracle wants the default before NOT NULL.
        if self.d == Dialect::Oracle {
            def.push_str(&default);
            def.push_str(null);
        } else {
            def.push_str(null);
            def.push_str(&default);
        }
        if self.d == Dialect::MySql {
            if let Some(extra) = extra {
                def.push(' ');
                def.push_str(extra);
            }
            if let Some(comment) = text(c.comment.as_ref()) {
                def.push_str(" COMMENT ");
                def.push_str(&self.d.quote_string(comment));
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
        let mut sql = format!(
            "CREATE TABLE {} (\n  {}\n)",
            self.table(name),
            parts.join(",\n  ")
        );
        if let Some(options) = text(design.options.as_ref()) {
            let _ = write!(sql, " {options}");
        }
        sql
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
        self.comments(table, &commented, None, script);
        if text(design.comment.as_ref()).is_some() {
            self.push_table_comment(design, false, script);
        }
    }

    /// The value of `COMMENT ON .. IS`: a string literal, or what clears the comment (Oracle,
    /// Db2 and Snowflake want an empty string, the others NULL).
    fn comment_value(&self, comment: Option<&str>) -> String {
        match comment {
            Some(t) => self.d.quote_string(t),
            None if matches!(self.d, Dialect::Oracle | Dialect::Db2 | Dialect::Snowflake) => {
                "''".to_owned()
            }
            None => "NULL".to_owned(),
        }
    }

    /// Adds the statement for the comment of `design`, if the database has table comments.
    fn push_table_comment(&self, design: &TableDesign, existed: bool, script: &mut Script) {
        match self.table_comment(&design.name, text(design.comment.as_ref()), existed) {
            Some(statement) => script.statements.push(statement),
            None => script
                .warnings
                .push("SQLite has no table comments: it is not saved.".to_owned()),
        }
    }

    /// The statement that sets (or with `None` removes) the comment of `table`; `None` where
    /// tables have no comments. `existed`: whether it had one, for SQL Server.
    fn table_comment(&self, table: &str, comment: Option<&str>, existed: bool) -> Option<String> {
        let lit = |t: &str| self.d.quote_string(t);
        let t = self.table(table);
        match self.d {
            Dialect::Postgres
            | Dialect::DuckDb
            | Dialect::Ansi
            | Dialect::Snowflake
            | Dialect::Oracle
            | Dialect::Db2
            | Dialect::Databricks => Some(format!(
                "COMMENT ON TABLE {t} IS {}",
                self.comment_value(comment)
            )),
            Dialect::MySql => Some(format!(
                "ALTER TABLE {t} COMMENT = {}",
                lit(comment.unwrap_or(""))
            )),
            Dialect::ClickHouse => Some(format!(
                "ALTER TABLE {t} MODIFY COMMENT {}",
                lit(comment.unwrap_or(""))
            )),
            Dialect::BigQuery => Some(format!(
                "ALTER TABLE {t} SET OPTIONS (description = {})",
                comment.map_or_else(|| "NULL".to_owned(), lit)
            )),
            Dialect::SqlServer => Some(self.mssql_description(table, None, comment, existed)),
            Dialect::Sqlite => None,
        }
    }

    /// `MS_Description` of `table` (or its `column`): added, updated or dropped.
    fn mssql_description(
        &self,
        table: &str,
        column: Option<&str>,
        comment: Option<&str>,
        existed: bool,
    ) -> String {
        let lit = |t: &str| self.d.quote_string(t);
        let mut target = format!("'SCHEMA', {}, 'TABLE', {}", lit(self.schema), lit(table));
        if let Some(c) = column {
            let _ = write!(target, ", 'COLUMN', {}", lit(c));
        }
        match (comment, existed) {
            (None, _) => format!("EXEC sp_dropextendedproperty 'MS_Description', {target}"),
            (Some(c), false) => format!(
                "EXEC sp_addextendedproperty 'MS_Description', {}, {target}",
                lit(c)
            ),
            (Some(c), true) => format!(
                "EXEC sp_updateextendedproperty 'MS_Description', {}, {target}",
                lit(c)
            ),
        }
    }

    /// Sets the comments of `columns`; MySQL has them in the column definitions already.
    /// `original` is the table as it was, to know whether a column had a comment.
    fn comments(
        &self,
        table: &str,
        columns: &[&ColumnDesign],
        original: Option<&TableDesign>,
        script: &mut Script,
    ) {
        if columns.is_empty() {
            return;
        }
        match self.d {
            // Snowflake, Oracle and Db2 take the standard statement.
            Dialect::Postgres
            | Dialect::DuckDb
            | Dialect::Ansi
            | Dialect::Snowflake
            | Dialect::Oracle
            | Dialect::Db2 => {
                for c in columns {
                    let comment = self.comment_value(text(c.comment.as_ref()));
                    script.statements.push(format!(
                        "COMMENT ON COLUMN {}.{} IS {comment}",
                        self.table(table),
                        self.q(&c.name)
                    ));
                }
            }
            Dialect::ClickHouse => {
                for c in columns {
                    let comment = self.d.quote_string(text(c.comment.as_ref()).unwrap_or(""));
                    script.statements.push(format!(
                        "ALTER TABLE {} COMMENT COLUMN {} {comment}",
                        self.table(table),
                        self.q(&c.name)
                    ));
                }
            }
            Dialect::MySql => {}
            Dialect::BigQuery => {
                for c in columns {
                    let d = text(c.comment.as_ref())
                        .map_or_else(|| "NULL".to_owned(), |t| self.d.quote_string(t));
                    script.statements.push(format!(
                        "ALTER TABLE {} ALTER COLUMN {} SET OPTIONS (description = {d})",
                        self.table(table),
                        self.q(&c.name)
                    ));
                }
            }
            Dialect::Databricks => {
                for c in columns {
                    let d = self.d.quote_string(text(c.comment.as_ref()).unwrap_or(""));
                    script.statements.push(format!(
                        "ALTER TABLE {} ALTER COLUMN {} COMMENT {d}",
                        self.table(table),
                        self.q(&c.name)
                    ));
                }
            }
            Dialect::SqlServer => {
                for c in columns {
                    let existed = original.is_some_and(|o| {
                        o.columns.iter().any(|oc| {
                            c.original.is_some()
                                && oc.original == c.original
                                && text(oc.comment.as_ref()).is_some()
                        })
                    });
                    script.statements.push(self.mssql_description(
                        table,
                        Some(&c.name),
                        text(c.comment.as_ref()),
                        existed,
                    ));
                }
            }
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
        let object = d.quote_string(&self.table(table));
        let drop = d.quote_string(&format!(
            "ALTER TABLE {} DROP CONSTRAINT ",
            self.table(table)
        ));
        let inner = format!(
            "DECLARE @n sysname = (SELECT name FROM sys.default_constraints \
             WHERE parent_object_id = OBJECT_ID({object}) \
             AND parent_column_id = COLUMNPROPERTY(OBJECT_ID({object}), {}, 'ColumnId')); \
             DECLARE @s nvarchar(max) = {drop} + QUOTENAME(@n); IF @n IS NOT NULL EXEC(@s)",
            d.quote_string(column)
        );
        format!("EXEC({})", d.quote_string(&inner))
    }

    fn alter(
        &self,
        old: &TableDesign,
        new: &TableDesign,
        diff: &Diff<'_>,
        script: &mut Script,
        keys: Option<&mut Keys>,
    ) {
        // A renamed table's keys stay in its script, after the rename.
        let mut keys = keys.filter(|_| new.name == old.name);
        let out = &mut script.statements;
        let mut name = old.name.as_str();
        if new.name != old.name {
            out.push(self.rename_table(name, &new.name));
            name = &new.name;
        }
        let t = self.table(name);
        for f in &diff.foreign_keys.dropped {
            let original = self.q(f.original.as_deref().unwrap_or(&f.name));
            let drop = match self.d {
                Dialect::MySql => format!("ALTER TABLE {t} DROP FOREIGN KEY {original}"),
                _ => format!("ALTER TABLE {t} DROP CONSTRAINT {original}"),
            };
            keys.as_deref_mut()
                .map_or(&mut *out, |k| &mut k.dropped)
                .push(drop);
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
            if self.d == Dialect::Oracle {
                out.push(format!("ALTER TABLE {t} ADD ({})", self.column(c)));
                continue;
            }
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
            let add = format!("ALTER TABLE {t} ADD {}", self.foreign_key(f));
            keys.as_deref_mut()
                .map_or(&mut *out, |k| &mut k.added)
                .push(add);
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
        self.comments(name, &commented, Some(old), script);
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
                    self.d.quote_string(&format!("{t}.{}", self.q(&o.name))),
                    self.d.quote_string(&n.name)
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
        if self.d == Dialect::ClickHouse {
            Self::alter_clickhouse_column(&t, &c, o, n, out);
            return;
        }
        if self.d == Dialect::Oracle {
            Self::alter_oracle_column(&t, &c, o, n, out);
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

    /// Oracle's `MODIFY`, one change at a time: setting a column to the nullability it has
    /// already is an error there.
    fn alter_oracle_column(
        table: &str,
        column: &str,
        old: &ColumnDesign,
        new: &ColumnDesign,
        out: &mut Vec<String>,
    ) {
        let modify = |what: &str| format!("ALTER TABLE {table} MODIFY ({column} {what})");
        if text(old.extra.as_ref()).is_some() && old.extra != new.extra {
            out.push(modify("DROP IDENTITY"));
        }
        if type_changed(old, new) {
            out.push(modify(new.data_type.trim()));
        }
        if default_changed(old, new) {
            let default = text(new.default.as_ref()).unwrap_or("NULL");
            out.push(modify(&format!("DEFAULT {default}")));
        }
        if old.nullable != new.nullable {
            out.push(modify(if new.nullable { "NULL" } else { "NOT NULL" }));
        }
        if let Some(identity) = text(new.extra.as_ref()).filter(|_| old.extra != new.extra) {
            out.push(modify(identity));
        }
    }

    /// `MODIFY COLUMN` with the type (nullability is part of it), which keeps the column's
    /// default and comment unless the default changes too.
    fn alter_clickhouse_column(
        table: &str,
        column: &str,
        old: &ColumnDesign,
        new: &ColumnDesign,
        out: &mut Vec<String>,
    ) {
        let (t, c) = (table, column);
        let retyped = type_changed(old, new) || old.nullable != new.nullable;
        let redefault = default_changed(old, new);
        if !retyped && !redefault {
            return;
        }
        let ty = clickhouse_type(new);
        match text(new.default.as_ref()).filter(|_| redefault) {
            Some(d) if clickhouse_default_kind(d) => {
                out.push(format!("ALTER TABLE {t} MODIFY COLUMN {c} {ty} {d}"));
            }
            Some(d) => out.push(format!(
                "ALTER TABLE {t} MODIFY COLUMN {c} {ty} DEFAULT {d}"
            )),
            None => {
                if retyped {
                    out.push(format!("ALTER TABLE {t} MODIFY COLUMN {c} {ty}"));
                }
                if redefault {
                    out.push(format!("ALTER TABLE {t} MODIFY COLUMN {c} REMOVE DEFAULT"));
                }
            }
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
            // A generated column computes its values again.
            let (from, to): (Vec<String>, Vec<String>) = diff
                .kept_columns
                .iter()
                .filter(|(_, n)| text(n.generated.as_ref()).is_none())
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
        script.warnings.push(format!(
            "{database} cannot make these changes in place: the table is created again and its rows copied."
        ));
        let mut from_design = Vec::new();
        for i in &new.indexes {
            match self.index_as_written(old, new, diff, i) {
                Some(sql) => script.statements.push(sql),
                // The drivers list an expression index's key parts as `(expression)`.
                None if i.columns.iter().any(|c| c == "(expression)") => {
                    script.warnings.push(format!(
                        "Index {} is on an expression: create it again by hand.",
                        i.name
                    ));
                }
                None => from_design.push(i.clone()),
            }
        }
        let design = TableDesign {
            indexes: from_design,
            ..new.clone()
        };
        self.indexes_and_comments(&design, &new.name, script);
        self.recreate_triggers(old, new, diff, script);
    }

    /// The `CREATE INDEX` the database keeps of an index the design leaves as it was (its
    /// columns not renamed either), on the table's new name: it keeps what the design does
    /// not hold (a `WHERE`, a collation, an expression).
    fn index_as_written(
        &self,
        old: &TableDesign,
        new: &TableDesign,
        diff: &Diff<'_>,
        i: &IndexDesign,
    ) -> Option<String> {
        let sql = text(i.sql.as_ref())?;
        let changed = diff.indexes.added.iter().any(|a| std::ptr::eq(*a, i));
        let renamed_columns = diff
            .kept_columns
            .iter()
            .any(|(o, n)| o.name != n.name && i.columns.contains(&n.name));
        (!changed && !renamed_columns).then(|| self.on_table(sql, &old.name, &new.name))
    }

    /// Creates the table's triggers (SQLite) again after a rebuild dropped them.
    fn recreate_triggers(
        &self,
        old: &TableDesign,
        new: &TableDesign,
        diff: &Diff<'_>,
        script: &mut Script,
    ) {
        if new.triggers.is_empty() {
            return;
        }
        for trigger in &new.triggers {
            script
                .statements
                .push(self.on_table(trigger, &old.name, &new.name));
        }
        let columns_changed = !diff.dropped_columns.is_empty()
            || diff.kept_columns.iter().any(|(o, n)| o.name != n.name);
        script.warnings.push(if columns_changed {
            "Its triggers are created again as they were: check those that use a renamed or dropped column."
                .to_owned()
        } else {
            "Its triggers are created again.".to_owned()
        });
    }

    /// `sql` (a `CREATE INDEX` or `CREATE TRIGGER`) with the table after its first `ON`
    /// renamed from `from` to `to`.
    fn on_table(&self, sql: &str, from: &str, to: &str) -> String {
        if from == to {
            return sql.to_owned();
        }
        let tokens: Vec<Token> = Lexer::new(sql, self.d).filter(|t| !t.is_trivia()).collect();
        let on = tokens
            .iter()
            .position(|t| t.kind == TokenKind::Word && t.text(sql).eq_ignore_ascii_case("ON"));
        match on.and_then(|i| tokens.get(i + 1)) {
            Some(name) => format!("{}{}{}", &sql[..name.start], self.q(to), &sql[name.end..]),
            None => sql.to_owned(),
        }
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
                sql: None,
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
    fn comments_clickhouse_columns() {
        let mut design = orders();
        design.columns[2].comment = Some("free text".to_owned());
        let s = script(Dialect::ClickHouse, "shop", None, &design).unwrap();
        assert_eq!(
            s.statements.last().map(String::as_str),
            Some("ALTER TABLE shop.orders COMMENT COLUMN note 'free text'"),
            "{s:?}"
        );
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
        assert!(
            !script(Dialect::ClickHouse, "shop", Some(&old), &new)
                .unwrap()
                .transactional
        );
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
    fn sqlite_rebuild_keeps_what_the_editor_does_not_show() {
        let mut old = orders();
        old.columns.push(ColumnDesign {
            generated: Some("GENERATED ALWAYS AS (total * 2) VIRTUAL".to_owned()),
            ..column("double", "numeric")
        });
        old.columns[2].collation = Some("NOCASE".to_owned());
        old.options = Some("WITHOUT ROWID, STRICT".to_owned());
        old.indexes[0].sql =
            Some("CREATE INDEX orders_total ON orders (total) WHERE total > 0".to_owned());
        old.triggers =
            vec!["CREATE TRIGGER audit AFTER INSERT ON \"orders\" BEGIN SELECT 1; END".to_owned()];
        let mut new = old.clone();
        new.columns[1].data_type = "real".to_owned();
        new.name = "purchases".to_owned();
        let s = script(Dialect::Sqlite, "main", Some(&old), &new).unwrap();
        assert_eq!(
            s.statements,
            [
                "CREATE TABLE main._platypus_orders AS SELECT * FROM main.orders",
                "DROP TABLE main.orders",
                "CREATE TABLE main.purchases (\n  id integer NOT NULL,\n  total real,\n  \
                 note text COLLATE NOCASE,\n  double numeric GENERATED ALWAYS AS (total * 2) VIRTUAL,\n  \
                 PRIMARY KEY (id)\n) WITHOUT ROWID, STRICT",
                "INSERT INTO main.purchases (id, total, note) SELECT id, total, note FROM main._platypus_orders",
                "DROP TABLE main._platypus_orders",
                "CREATE INDEX orders_total ON purchases (total) WHERE total > 0",
                "CREATE TRIGGER audit AFTER INSERT ON purchases BEGIN SELECT 1; END",
            ]
        );
        assert!(
            s.warnings
                .iter()
                .any(|w| w == "Its triggers are created again."),
            "{s:?}"
        );

        // An index on a renamed column is written from the design.
        let mut new = old.clone();
        new.columns[1].name = "amount".to_owned();
        new.columns[0].data_type = "int".to_owned();
        new.indexes[0].columns = vec!["amount".to_owned()];
        let s = script(Dialect::Sqlite, "main", Some(&old), &new).unwrap();
        assert!(
            s.statements
                .contains(&"CREATE INDEX main.orders_total ON orders (amount)".to_owned()),
            "{s:?}"
        );
        assert!(
            s.warnings.iter().any(|w| w.contains("renamed or dropped")),
            "{s:?}"
        );
    }

    #[test]
    fn mysql_redefines_generated_columns_with_their_clause() {
        let mut old = orders();
        old.columns.push(ColumnDesign {
            generated: Some("GENERATED ALWAYS AS ((`total` * 2)) VIRTUAL".to_owned()),
            collation: None,
            ..column("dbl", "int")
        });
        let mut new = old.clone();
        new.columns[3].comment = Some("doubled".to_owned());
        new.columns[2].collation = Some("utf8mb4_bin".to_owned());
        let s = script(Dialect::MySql, "shop", Some(&old), &new).unwrap();
        assert_eq!(
            s.statements,
            [
                "ALTER TABLE shop.orders CHANGE COLUMN note note text COLLATE utf8mb4_bin",
                "ALTER TABLE shop.orders CHANGE COLUMN dbl dbl int GENERATED ALWAYS AS ((`total` * 2)) VIRTUAL COMMENT 'doubled'",
            ]
        );
    }

    #[test]
    fn clickhouse_modifies_columns_and_warns_it_is_not_atomic() {
        let old = TableDesign {
            name: "t".to_owned(),
            columns: vec![
                ColumnDesign {
                    nullable: false,
                    ..column("id", "Int32")
                },
                column("qty", "Nullable(Int32)"),
                ColumnDesign {
                    default: Some("5".to_owned()),
                    ..column("n", "Nullable(Int32)")
                },
            ],
            ..TableDesign::default()
        };
        let mut new = old.clone();
        new.columns[1].name = "amount".to_owned();
        new.columns[1].data_type = "Nullable(Int64)".to_owned();
        new.columns[0].nullable = true;
        new.columns[2].default = None;
        let s = script(Dialect::ClickHouse, "db", Some(&old), &new).unwrap();
        assert_eq!(
            s.statements,
            [
                "ALTER TABLE db.t MODIFY COLUMN id Nullable(Int32)",
                "ALTER TABLE db.t RENAME COLUMN qty TO amount",
                "ALTER TABLE db.t MODIFY COLUMN amount Nullable(Int64)",
                "ALTER TABLE db.t MODIFY COLUMN n REMOVE DEFAULT",
            ]
        );
        assert!(!s.transactional);
        assert!(
            s.warnings[0].starts_with("ClickHouse runs each statement"),
            "{s:?}"
        );

        let mut new = old.clone();
        new.columns[2].default = Some("7".to_owned());
        new.columns.push(ColumnDesign {
            original: None,
            default: Some("MATERIALIZED id * 2".to_owned()),
            ..column("m", "Int64")
        });
        let s = script(Dialect::ClickHouse, "db", Some(&old), &new).unwrap();
        assert_eq!(
            s.statements,
            [
                "ALTER TABLE db.t MODIFY COLUMN n Nullable(Int32) DEFAULT 7",
                "ALTER TABLE db.t ADD COLUMN m Nullable(Int64) MATERIALIZED id * 2",
            ]
        );
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

    #[test]
    fn sqlite_columns_can_have_no_type() {
        let mut design = orders();
        design.columns[2].data_type = String::new();
        let created = statements(Dialect::Sqlite, None, &design);
        assert!(
            created[0].contains("note,") || created[0].contains("note)"),
            "{created:?}"
        );
        assert!(!created[0].contains("note "), "{created:?}");
        // An unchanged table needs nothing, a changed one is rebuilt with the typeless column.
        assert_eq!(
            statements(Dialect::Sqlite, Some(&design), &design),
            Vec::<String>::new()
        );
        let mut changed = design.clone();
        changed.columns[1].data_type = "real".to_owned();
        let rebuilt = statements(Dialect::Sqlite, Some(&design), &changed);
        assert!(
            rebuilt.iter().any(|s| s.starts_with("CREATE TABLE")
                && s.contains("total real")
                && !s.contains("note ")),
            "{rebuilt:?}"
        );
    }

    fn commented(table: Option<&str>, column: Option<&str>) -> TableDesign {
        let mut d = orders();
        d.comment = table.map(str::to_owned);
        d.columns[2].comment = column.map(str::to_owned);
        d
    }

    /// The statements for going from `old` to `new` (`None`: creating the table).
    fn statements(d: Dialect, old: Option<&TableDesign>, new: &TableDesign) -> Vec<String> {
        script(d, "public", old, new).unwrap().statements
    }

    fn assert_has(d: Dialect, got: &[String], expected: &str) {
        assert!(
            got.iter().any(|s| s == expected),
            "{d:?}: {expected}\nin {got:#?}"
        );
    }

    #[test]
    fn table_and_column_comments_per_dialect() {
        let cases: &[(Dialect, &str, &str)] = &[
            (
                Dialect::Postgres,
                "COMMENT ON TABLE public.orders IS 'All orders'",
                "COMMENT ON COLUMN public.orders.note IS 'free text'",
            ),
            (
                Dialect::Oracle,
                "COMMENT ON TABLE \"public\".\"orders\" IS 'All orders'",
                "COMMENT ON COLUMN \"public\".\"orders\".\"note\" IS 'free text'",
            ),
            (
                Dialect::Db2,
                "COMMENT ON TABLE \"public\".\"orders\" IS 'All orders'",
                "COMMENT ON COLUMN \"public\".\"orders\".\"note\" IS 'free text'",
            ),
            (
                Dialect::MySql,
                "ALTER TABLE public.orders COMMENT = 'All orders'",
                "ALTER TABLE public.orders CHANGE COLUMN note note text COMMENT 'free text'",
            ),
            (
                Dialect::ClickHouse,
                "ALTER TABLE public.orders MODIFY COMMENT 'All orders'",
                "ALTER TABLE public.orders COMMENT COLUMN note 'free text'",
            ),
            (
                Dialect::SqlServer,
                "EXEC sp_addextendedproperty 'MS_Description', N'All orders', 'SCHEMA', N'public', 'TABLE', N'orders'",
                "EXEC sp_addextendedproperty 'MS_Description', N'free text', 'SCHEMA', N'public', 'TABLE', N'orders', 'COLUMN', N'note'",
            ),
            (
                Dialect::BigQuery,
                "ALTER TABLE public.orders SET OPTIONS (description = 'All orders')",
                "ALTER TABLE public.orders ALTER COLUMN note SET OPTIONS (description = 'free text')",
            ),
            (
                Dialect::Databricks,
                "COMMENT ON TABLE public.orders IS 'All orders'",
                "ALTER TABLE public.orders ALTER COLUMN note COMMENT 'free text'",
            ),
        ];
        let old = commented(None, None);
        let new = commented(Some("All orders"), Some("free text"));
        for (d, table, column) in cases {
            let altered = statements(*d, Some(&old), &new);
            assert_has(*d, &altered, table);
            assert_has(*d, &altered, column);
            // Creating the table: MySQL has the column comment in its definition.
            assert_has(*d, &statements(*d, None, &new), table);
        }
    }

    #[test]
    fn removing_a_comment() {
        let old = commented(Some("All orders"), Some("free text"));
        let new = commented(None, None);
        let cases: &[(Dialect, &str, &str)] = &[
            (
                Dialect::Postgres,
                "COMMENT ON TABLE public.orders IS NULL",
                "COMMENT ON COLUMN public.orders.note IS NULL",
            ),
            (
                Dialect::Databricks,
                "COMMENT ON TABLE public.orders IS NULL",
                "ALTER TABLE public.orders ALTER COLUMN note COMMENT ''",
            ),
            (
                Dialect::Oracle,
                "COMMENT ON TABLE \"public\".\"orders\" IS ''",
                "COMMENT ON COLUMN \"public\".\"orders\".\"note\" IS ''",
            ),
            (
                Dialect::Db2,
                "COMMENT ON TABLE \"public\".\"orders\" IS ''",
                "COMMENT ON COLUMN \"public\".\"orders\".\"note\" IS ''",
            ),
            (
                Dialect::MySql,
                "ALTER TABLE public.orders COMMENT = ''",
                "ALTER TABLE public.orders CHANGE COLUMN note note text",
            ),
            (
                Dialect::ClickHouse,
                "ALTER TABLE public.orders MODIFY COMMENT ''",
                "ALTER TABLE public.orders COMMENT COLUMN note ''",
            ),
            (
                Dialect::BigQuery,
                "ALTER TABLE public.orders SET OPTIONS (description = NULL)",
                "ALTER TABLE public.orders ALTER COLUMN note SET OPTIONS (description = NULL)",
            ),
            (
                Dialect::SqlServer,
                "EXEC sp_dropextendedproperty 'MS_Description', 'SCHEMA', N'public', 'TABLE', N'orders'",
                "EXEC sp_dropextendedproperty 'MS_Description', 'SCHEMA', N'public', 'TABLE', N'orders', 'COLUMN', N'note'",
            ),
        ];
        for (d, table, column) in cases {
            let got = statements(*d, Some(&old), &new);
            assert_has(*d, &got, table);
            assert_has(*d, &got, column);
        }
        let changed = commented(Some("Other"), Some("changed"));
        let got = statements(Dialect::SqlServer, Some(&old), &changed);
        let target = "'SCHEMA', N'public', 'TABLE', N'orders'";
        let update = "EXEC sp_updateextendedproperty 'MS_Description', N";
        assert_has(
            Dialect::SqlServer,
            &got,
            &format!("{update}'Other', {target}"),
        );
        assert_has(
            Dialect::SqlServer,
            &got,
            &format!("{update}'changed', {target}, 'COLUMN', N'note'"),
        );
    }

    #[test]
    fn comments_with_quotes_and_backslashes_are_escaped() {
        let new = commented(Some(r"it's a\b"), Some(r"it's a\b"));
        let cases: &[(Dialect, &str)] = &[
            (Dialect::Postgres, r"E'it''s a\\b'"),
            (Dialect::Oracle, r"'it''s a\b'"),
            (Dialect::Db2, r"'it''s a\b'"),
            (Dialect::MySql, r"'it''s a\\b'"),
            (Dialect::ClickHouse, r"'it''s a\\b'"),
            (Dialect::BigQuery, r"'it\'s a\\b'"),
            (Dialect::Databricks, r"'it\'s a\\b'"),
            (Dialect::SqlServer, r"N'it''s a\b'"),
        ];
        for (d, literal) in cases {
            let text = script(*d, "public", None, &new).unwrap().text(*d);
            assert!(text.matches(literal).count() >= 2, "{d:?}: {text}");
        }
    }

    #[test]
    fn sqlite_warns_that_table_comments_are_not_saved() {
        let table = commented(Some("All orders"), None);
        let s = script(Dialect::Sqlite, "main", None, &table).unwrap();
        assert!(
            s.warnings.iter().any(|w| w.contains("table comments")),
            "{s:?}"
        );
        assert!(!s.text(Dialect::Sqlite).contains("All orders"), "{s:?}");
    }

    #[test]
    fn checks_postgresql_prints_another_way_are_the_same() {
        let given = "((status)::text = ANY ((ARRAY['new'::character varying, \
                     'paid'::character varying])::text[]))";
        let printed = "((status)::text = ANY (ARRAY[('new'::character varying)::text, \
                       ('paid'::character varying)::text]))";
        assert!(same_check(given, printed));
        assert!(same_check("(total >= 0)", "total>=0"));
        assert!(same_check(
            "(created > '2020-01-01'::timestamp without time zone)",
            "created > '2020-01-01'"
        ));
        assert!(!same_check(given, &given.replace("paid", "sent")));
        assert!(!same_check("a = 'X'", "a = 'x'"));
        assert!(!same_check("total >= 0", "total > 0"));
        assert!(same_check(
            "([status]='new' OR [status]='paid')",
            "[status] = 'new' or [status] = 'paid'"
        ));
    }
}
