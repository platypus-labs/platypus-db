//! Migration of tables between databases: each is created in the target with its columns
//! mapped (types, NOT NULL, primary key, identity) and its rows copied; then come the column
//! defaults, the secondary indexes, the foreign keys between the copied tables and the CHECK
//! constraints, and, between databases of one kind, the views and routines.
//!
//! Whatever cannot follow (a default or CHECK written in another dialect, a key SQLite cannot
//! add to an existing table) is left out with a warning: the tables and rows are there anyway.

use std::collections::HashSet;

use arrow_schema::SchemaRef;
use platypus_core::{
    ColumnMeta, Connection, ConstraintKind, ExecOptions, ExecResult, ForeignKeyMeta, META_DB_TYPE,
    ObjectKind, ObjectRef, TableDetails,
};
use platypus_sql::{Dialect, Lexer, TokenKind};

use crate::import::{
    ImportColumn, ImportError, ImportMode, ImportPlan, import, query_rows, query_value, target_type,
};

/// What a migration did.
#[derive(Debug, Default)]
pub struct Migrated {
    pub rows: u64,
    /// What was left out, with why.
    pub skipped: Vec<String>,
}

/// One end of a migration: a connection, its dialect, and the schema of the tables.
pub type End<'a> = (&'a dyn Connection, Dialect, &'a str);

/// A source table, as read before the copy.
struct Table {
    name: String,
    details: TableDetails,
    /// The columns of `SELECT *`, with their Arrow types.
    fields: SchemaRef,
    /// The column the database numbers by itself: an integer primary key of its own.
    identity: Option<String>,
}

impl Table {
    fn primary_key(&self) -> Vec<String> {
        self.details
            .columns
            .iter()
            .filter(|c| c.primary_key)
            .map(|c| c.name.clone())
            .collect()
    }
}

/// A statement run after the rows, in a transaction of its own; `label` names it in the
/// warning when it fails.
struct Step {
    label: String,
    sql: String,
}

/// A SQL string literal.
fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn names(dialect: Dialect, names: &[String]) -> String {
    names
        .iter()
        .map(|n| dialect.quote_ident(n).into_owned())
        .collect::<Vec<_>>()
        .join(", ")
}

async fn read_table((source, dialect, schema): End<'_>, name: &str) -> Result<Table, ImportError> {
    let details = source.table_details(schema, name).await?;
    let sql = format!(
        "SELECT * FROM {} WHERE 1 = 0",
        dialect.qualified(Some(schema), name)
    );
    let ExecResult::Rows(rows) = source.execute(&sql, &ExecOptions::default()).await? else {
        return Err(ImportError::Plan(format!("{name} returned no rows")));
    };
    let fields = rows.schema();
    drop(rows);
    let mut table = Table {
        name: name.to_owned(),
        details,
        fields,
        identity: None,
    };
    table.identity = identity((source, dialect, schema), &table).await?;
    Ok(table)
}

/// The identity (serial, auto-increment) column of `table`, if it is the whole primary key:
/// the one place every database numbers rows the same way.
async fn identity(
    (source, dialect, schema): End<'_>,
    table: &Table,
) -> Result<Option<String>, ImportError> {
    let keys: Vec<&ColumnMeta> = table
        .details
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .collect();
    let [key] = keys[..] else {
        return Ok(None);
    };
    let declared = key.data_type.to_ascii_lowercase();
    if !(declared.contains("int") || declared.contains("serial")) {
        return Ok(None);
    }
    let serial = key
        .default
        .as_deref()
        .is_some_and(|d| d.starts_with("nextval("));
    let qualified = literal(&dialect.qualified(Some(schema), &table.name));
    let column = literal(&key.name);
    let query = match dialect {
        // An INTEGER PRIMARY KEY is the rowid.
        Dialect::Sqlite => return Ok((declared == "integer").then(|| key.name.clone())),
        Dialect::DuckDb => return Ok(serial.then(|| key.name.clone())),
        Dialect::Postgres if serial => return Ok(Some(key.name.clone())),
        Dialect::Postgres => format!(
            "SELECT attidentity FROM pg_attribute WHERE attrelid = {qualified}::regclass \
             AND attname = {column}"
        ),
        Dialect::MySql => format!(
            "SELECT EXTRA FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = {} \
             AND TABLE_NAME = {} AND COLUMN_NAME = {column}",
            literal(schema),
            literal(&table.name)
        ),
        Dialect::SqlServer => {
            format!("SELECT COLUMNPROPERTY(OBJECT_ID({qualified}), {column}, 'IsIdentity')")
        }
    };
    let value = query_value(source, &query).await?.unwrap_or_default();
    let found = match dialect {
        Dialect::MySql => value.contains("auto_increment"),
        Dialect::SqlServer => value == "1",
        _ => !value.is_empty(),
    };
    Ok(found.then(|| key.name.clone()))
}

/// `tables` with each after the tables its foreign keys point at, where a cycle allows it:
/// SQLite and DuckDB declare the keys with the table, and check them as rows arrive.
fn ordered(tables: Vec<Table>) -> Vec<Table> {
    let mut pending = tables;
    let mut done: Vec<Table> = Vec::with_capacity(pending.len());
    while !pending.is_empty() {
        let ready = |t: &Table| {
            t.details.foreign_keys.iter().all(|fk| {
                fk.ref_table == t.name
                    || done.iter().any(|d| d.name == fk.ref_table)
                    || !pending.iter().any(|p| p.name == fk.ref_table)
            })
        };
        // In a cycle, the first table goes anyway.
        let next = pending.iter().position(ready).unwrap_or(0);
        done.push(pending.remove(next));
    }
    done
}

/// What the target makes of a source column's default, if it can be written there: numbers,
/// strings, booleans and the current time.
fn portable_default(
    text: &str,
    (source, target): (Dialect, Dialect),
    declared: &str,
) -> Option<String> {
    let mut v = text.trim();
    loop {
        // SQL Server wraps its defaults in parentheses, PostgreSQL casts them: ((0)), 'a'::text.
        if let Some(inner) = v.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
            v = inner.trim();
        } else if let Some((value, _)) = v.rsplit_once("::").filter(|(_, t)| !t.contains('\'')) {
            v = value.trim();
        } else {
            break;
        }
    }
    let quoted = |s: &str| {
        s.len() >= 2
            && s.starts_with('\'')
            && s.ends_with('\'')
            && !s[1..s.len() - 1].replace("''", "").contains('\'')
    };
    let number = {
        let digits = v.strip_prefix('-').unwrap_or(v);
        !digits.is_empty()
            && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
            && digits.matches('.').count() <= 1
    };
    let declared = declared.to_ascii_lowercase();
    let text_column = ["char", "text", "enum", "set"]
        .iter()
        .any(|t| declared.contains(t));
    let upper = v.to_ascii_uppercase();
    Some(match upper.as_str() {
        _ if quoted(v) => v.to_owned(),
        // N'...' of SQL Server.
        _ if upper.starts_with("N'") && quoted(&v[1..]) => v[1..].to_owned(),
        // MySQL leaves the quotes out (MariaDB does not).
        _ if source == Dialect::MySql && text_column => literal(v),
        _ if number => v.to_owned(),
        "NULL" => "NULL".to_owned(),
        "TRUE" | "FALSE" if target == Dialect::SqlServer => {
            if upper == "TRUE" { "1" } else { "0" }.to_owned()
        }
        "TRUE" | "FALSE" => upper,
        "CURRENT_TIMESTAMP"
        | "CURRENT_TIMESTAMP()"
        | "NOW()"
        | "GETDATE()"
        | "SYSDATETIME()"
        | "LOCALTIMESTAMP" => "CURRENT_TIMESTAMP".to_owned(),
        "CURRENT_DATE" | "CURRENT_DATE()" | "CURDATE()" if target != Dialect::SqlServer => {
            "CURRENT_DATE".to_owned()
        }
        _ => return None,
    })
}

/// The constraint of a CHECK as `CHECK (...)`, whichever way the database prints it.
fn check_clause(definition: &str) -> String {
    let trimmed = definition.trim();
    if trimmed.to_ascii_uppercase().starts_with("CHECK") {
        trimmed.to_owned()
    } else {
        format!("CHECK ({trimmed})")
    }
}

/// Whether the target declares foreign keys and CHECKs only with the table.
const fn declares_with_table(dialect: Dialect) -> bool {
    matches!(dialect, Dialect::Sqlite | Dialect::DuckDb)
}

const fn database(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::Postgres => "PostgreSQL",
        Dialect::MySql => "MySQL",
        Dialect::Sqlite => "SQLite",
        Dialect::DuckDb => "DuckDB",
        Dialect::SqlServer => "SQL Server",
    }
}

/// Everything a migration knows besides the rows.
struct Migration<'a> {
    source: Dialect,
    target: Dialect,
    schema: &'a str,
    /// The tables being copied.
    names: HashSet<&'a str>,
    /// What follows the current table's rows, before the next table is made (DuckDB alters
    /// no table that a foreign key points at).
    now: Vec<Step>,
    /// The current table's defaults, after `now`: SQL Server changes no column with one.
    defaults: Vec<Step>,
    /// What follows every table: the foreign keys and CHECKs.
    end: Vec<Step>,
    skipped: Vec<String>,
}

impl Migration<'_> {
    fn table(&self, name: &str) -> String {
        self.target.qualified(Some(self.schema), name)
    }

    /// Whether the table `fk` points at is copied too; a warning under `label` if not.
    fn copied(&mut self, label: &str, fk: &ForeignKeyMeta) -> bool {
        let copied = self.names.contains(fk.ref_table.as_str());
        if !copied {
            self.skipped
                .push(format!("{label}: {} was not copied", fk.ref_table));
        }
        copied
    }

    /// The plan that creates `table` in the target, its constraints declared there when the
    /// target allows no other way; `created` are the tables made before it.
    fn plan(&mut self, table: &Table, created: &[&Table]) -> ImportPlan {
        let mut constraints = Vec::new();
        if declares_with_table(self.target) {
            constraints.extend(self.inline_foreign_keys(table, created));
            constraints.extend(self.inline_checks(table));
        }
        let mut columns = Vec::with_capacity(table.fields.fields().len());
        for f in table.fields.fields() {
            let meta = table.details.columns.iter().find(|c| &c.name == f.name());
            // The catalog's type, with its length, where a result's may have none (MySQL).
            let mut field = f.as_ref().clone();
            if let Some(m) = meta {
                let mut metadata = field.metadata().clone();
                metadata.insert(META_DB_TYPE.to_owned(), m.data_type.clone());
                field = field.with_metadata(metadata);
            }
            let data_type = target_type(&field, self.target);
            let default = meta.and_then(|m| self.default_of(table, m, &data_type));
            columns.push(ImportColumn {
                source: f.name().clone(),
                target: f.name().clone(),
                data_type: Some(data_type),
                not_null: meta.is_some_and(|m| !m.nullable),
                default,
            });
        }
        ImportPlan {
            source_table: table.name.clone(),
            source_schema: None,
            schema: Some(self.schema.to_owned()),
            table: table.name.clone(),
            create: true,
            mode: ImportMode::Append,
            keys: Vec::new(),
            primary_key: table.primary_key(),
            columns,
            identity: table.identity.clone(),
            constraints,
        }
    }

    /// The default of `column` declared with the table where foreign keys are (SQLite alters
    /// no column), or added after the rows; one the target cannot read is a warning.
    fn default_of(
        &mut self,
        table: &Table,
        column: &ColumnMeta,
        data_type: &str,
    ) -> Option<String> {
        let text = column.default.as_deref()?;
        if table.identity.as_deref() == Some(&column.name) {
            return None;
        }
        let label = format!("{}.{}", table.name, column.name);
        let Some(value) = portable_default(text, (self.source, self.target), &column.data_type)
        else {
            self.skipped
                .push(format!("{label}: the default {text} was not copied"));
            return None;
        };
        if value == "NULL" {
            return None;
        }
        if declares_with_table(self.target) {
            return Some(value);
        }
        let t = self.table(&table.name);
        let c = self.target.quote_ident(&column.name);
        let sql = match self.target {
            Dialect::SqlServer => format!("ALTER TABLE {t} ADD DEFAULT {value} FOR {c}"),
            // An expression default takes its parentheses, and a TEXT column only this way.
            Dialect::MySql => format!(
                "ALTER TABLE {t} MODIFY {c} {data_type} DEFAULT ({value}){}",
                if column.nullable { "" } else { " NOT NULL" }
            ),
            _ => format!("ALTER TABLE {t} ALTER COLUMN {c} SET DEFAULT {value}"),
        };
        self.defaults.push(Step {
            label: format!("{label}: default"),
            sql,
        });
        None
    }

    /// The foreign keys of `table` declared with it: to a table created before, on its
    /// primary key (the rows arrive before the other unique indexes).
    fn inline_foreign_keys(&mut self, table: &Table, created: &[&Table]) -> Vec<String> {
        let mut clauses = Vec::new();
        for fk in &table.details.foreign_keys {
            let label = format!("{}.{}", table.name, fk.name);
            if !self.copied(&label, fk) {
                continue;
            }
            let referenced = if fk.ref_table == table.name {
                Some(table)
            } else {
                created.iter().copied().find(|t| t.name == fk.ref_table)
            };
            let on_key = referenced.is_some_and(|r| {
                let mut key = r.primary_key();
                let mut columns = fk.ref_columns.clone();
                key.sort();
                columns.sort();
                key == columns
            });
            // DuckDB has no key on its own table.
            let own = fk.ref_table == table.name && self.target == Dialect::DuckDb;
            if referenced.is_none() || !on_key || own {
                self.skipped.push(format!(
                    "{label}: {} declares foreign keys only with the table, to the primary key \
                     of a table created before",
                    database(self.target)
                ));
                continue;
            }
            // SQLite names the referenced table without its schema.
            let referenced = if self.target == Dialect::Sqlite {
                self.target.quote_ident(&fk.ref_table).into_owned()
            } else {
                self.table(&fk.ref_table)
            };
            clauses.push(format!(
                "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {referenced} ({})",
                self.target.quote_ident(&fk.name),
                names(self.target, &fk.columns),
                names(self.target, &fk.ref_columns)
            ));
        }
        clauses
    }

    /// The CHECKs of `table` declared with it: only from a database of the same kind, as a
    /// failed one would fail the table.
    fn inline_checks(&mut self, table: &Table) -> Vec<String> {
        let mut clauses = Vec::new();
        for c in checks(&table.details) {
            if self.source == self.target {
                clauses.push(format!(
                    "CONSTRAINT {} {}",
                    self.target.quote_ident(&c.name),
                    check_clause(&c.definition)
                ));
            } else {
                self.skipped.push(format!(
                    "{}.{}: {} adds no CHECK to an existing table, and this one is written for {}",
                    table.name,
                    c.name,
                    database(self.target),
                    database(self.source)
                ));
            }
        }
        clauses
    }

    /// The indexes, foreign keys and CHECKs added after the rows of `table`; its column types
    /// in the target are `plan`'s.
    fn after_rows(&mut self, table: &Table, plan: &ImportPlan) {
        self.indexes(table, plan);
        if declares_with_table(self.target) {
            return;
        }
        let t = self.table(&table.name);
        for fk in &table.details.foreign_keys {
            let label = format!("{}.{}", table.name, fk.name);
            if !self.copied(&label, fk) {
                continue;
            }
            let sql = foreign_key_sql(self.target, self.schema, &table.name, fk);
            self.end.push(Step { label, sql });
        }
        for c in checks(&table.details) {
            let sql = format!(
                "ALTER TABLE {t} ADD CONSTRAINT {} {}",
                self.target.quote_ident(&c.name),
                check_clause(&requote(&c.definition, (self.source, self.target)))
            );
            let label = format!("{}.{}", table.name, c.name);
            self.end.push(Step { label, sql });
        }
    }

    /// The secondary indexes of `table`, unique constraints included; an index of
    /// expressions stays behind.
    fn indexes(&mut self, table: &Table, plan: &ImportPlan) {
        let mut seen: HashSet<(Vec<String>, bool)> = HashSet::new();
        let unique_constraints = table
            .details
            .constraints
            .iter()
            .filter(|c| c.kind == ConstraintKind::Unique)
            .map(|c| (c.name.clone(), c.columns.clone(), true));
        let indexes = table
            .details
            .indexes
            .iter()
            .filter(|i| !i.primary)
            .map(|i| (i.name.clone(), i.columns.clone(), i.unique))
            .chain(unique_constraints);
        for (name, columns, unique) in indexes.collect::<Vec<_>>() {
            if columns.is_empty() || !seen.insert((columns.clone(), unique)) {
                continue;
            }
            let label = format!("{}.{name}", table.name);
            let targets: Option<Vec<&ImportColumn>> = columns
                .iter()
                .map(|c| plan.columns.iter().find(|p| &p.target == c))
                .collect();
            let Some(targets) = targets else {
                self.skipped
                    .push(format!("{label}: an index of expressions is not copied"));
                continue;
            };
            // SQLite names the indexes of its UNIQUE constraints, and keeps those names.
            let name = if name.starts_with("sqlite_autoindex") {
                format!("{}_{}_key", table.name, columns.join("_"))
            } else {
                name
            };
            let list: Vec<String> = targets
                .iter()
                .map(|c| self.indexed_column(&table.name, c))
                .collect();
            let (index, on) = if self.target == Dialect::Sqlite {
                // SQLite qualifies the index, not its table.
                let on = self.target.quote_ident(&table.name).into_owned();
                (self.target.qualified(Some(self.schema), &name), on)
            } else {
                let index = self.target.quote_ident(&name).into_owned();
                (index, self.table(&table.name))
            };
            let sql = format!(
                "CREATE {}INDEX {index} ON {on} ({})",
                if unique { "UNIQUE " } else { "" },
                list.join(", ")
            );
            self.now.push(Step { label, sql });
        }
    }

    /// `column` as an index names it: MySQL indexes a prefix of TEXT, and SQL Server no
    /// NVARCHAR(MAX), which is shortened first to what an index key holds.
    fn indexed_column(&mut self, table: &str, column: &ImportColumn) -> String {
        let q = self.target.quote_ident(&column.target).into_owned();
        let declared = column.data_type.as_deref().unwrap_or_default();
        match self.target {
            Dialect::MySql if declared.eq_ignore_ascii_case("TEXT") => format!("{q}(255)"),
            Dialect::SqlServer if declared.eq_ignore_ascii_case("NVARCHAR(MAX)") => {
                let sql = format!(
                    "ALTER TABLE {} ALTER COLUMN {q} NVARCHAR(450) {}",
                    self.table(table),
                    if column.not_null { "NOT NULL" } else { "NULL" }
                );
                let label = format!("{table}.{}: shortened for an index", column.target);
                self.now.push(Step { label, sql });
                q
            }
            _ => q,
        }
    }
}

fn checks(details: &TableDetails) -> impl Iterator<Item = &platypus_core::ConstraintMeta> {
    details
        .constraints
        .iter()
        .filter(|c| c.kind == ConstraintKind::Check)
}

/// `ALTER TABLE ... ADD CONSTRAINT ... FOREIGN KEY` of `fk` on `table`, in the target schema.
fn foreign_key_sql(dialect: Dialect, schema: &str, table: &str, fk: &ForeignKeyMeta) -> String {
    format!(
        "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
        dialect.qualified(Some(schema), table),
        dialect.quote_ident(&fk.name),
        names(dialect, &fk.columns),
        dialect.qualified(Some(schema), &fk.ref_table),
        names(dialect, &fk.ref_columns)
    )
}

/// Runs `sql` in a transaction of its own, so that a failure aborts nothing else.
async fn apply(conn: &dyn Connection, sql: &str) -> Result<(), platypus_core::Error> {
    conn.execute("BEGIN", &ExecOptions::default()).await?;
    // Read to the end: a `SELECT setval(...)` runs as its rows are read.
    match query_rows(conn, sql).await {
        Ok(_) => conn.commit().await,
        Err(e) => {
            let _ = conn.rollback().await;
            Err(match e {
                ImportError::Database(e) => e,
                e => platypus_core::Error::Other(e.to_string()),
            })
        }
    }
}

/// Runs `steps`, each in a transaction of its own; the failures become warnings.
async fn apply_all(conn: &dyn Connection, steps: Vec<Step>, skipped: &mut Vec<String>) {
    for step in steps {
        if let Err(e) = apply(conn, &step.sql).await {
            skipped.push(format!("{}: {e}", step.label));
        }
    }
}

/// The statements that number new rows after the copied ones, where the identity column does
/// not follow the values inserted into it.
async fn identity_steps(
    (target, dialect, schema): End<'_>,
    table: &str,
    column: &str,
) -> Result<Vec<Step>, ImportError> {
    let t = dialect.qualified(Some(schema), table);
    let c = dialect.quote_ident(column);
    let label = format!("{table}.{column}: numbering");
    Ok(match dialect {
        Dialect::Postgres => vec![Step {
            label,
            sql: format!(
                "SELECT setval(pg_get_serial_sequence({}, {}), COALESCE(MAX({c}), 0) + 1, false) \
                 FROM {t}",
                literal(&t),
                literal(column)
            ),
        }],
        Dialect::DuckDb => {
            let max = query_value(target, &format!("SELECT MAX({c}) FROM {t}"))
                .await?
                .and_then(|m| m.parse::<i64>().ok())
                .unwrap_or(0);
            let sequence = dialect.qualified(Some(schema), &format!("{table}_{column}_seq"));
            vec![
                Step {
                    label: label.clone(),
                    sql: format!("CREATE SEQUENCE {sequence} START WITH {}", max + 1),
                },
                Step {
                    label,
                    sql: format!(
                        "ALTER TABLE {t} ALTER COLUMN {c} SET DEFAULT nextval({})",
                        literal(&sequence)
                    ),
                },
            ]
        }
        // They number after the largest value, or are the rowid.
        Dialect::MySql | Dialect::SqlServer | Dialect::Sqlite => Vec::new(),
    })
}

/// Copies `tables` of the source schema into new tables of the target schema, one table per
/// transaction, then adds their defaults, indexes, foreign keys and CHECKs; with `objects`,
/// also the schema's views, functions, procedures and triggers. `progress` gets the rows
/// copied so far and returns false to stop. A failed table stops the migration; the tables
/// copied before it stay.
pub async fn migrate(
    source: End<'_>,
    (target, dialect, schema): End<'_>,
    tables: &[String],
    objects: bool,
    mut progress: impl FnMut(u64) -> bool,
) -> Result<Migrated, ImportError> {
    let mut read = Vec::with_capacity(tables.len());
    for name in tables {
        read.push(read_table(source, name).await?);
    }
    let read = ordered(read);
    let mut m = Migration {
        source: source.1,
        target: dialect,
        schema,
        names: tables.iter().map(String::as_str).collect(),
        now: Vec::new(),
        defaults: Vec::new(),
        end: Vec::new(),
        skipped: Vec::new(),
    };
    let mut rows = 0;
    for (i, table) in read.iter().enumerate() {
        let created: Vec<&Table> = read[..i].iter().collect();
        let mut plan = m.plan(table, &created);
        plan.source_schema = Some(source.2.to_owned());
        let base = rows;
        rows += import((source.0, source.1), (target, dialect), &plan, |n| {
            progress(base + n)
        })
        .await
        .map_err(|e| match e {
            ImportError::Cancelled => e,
            e => ImportError::Plan(format!("{}: {e}", table.name)),
        })?;
        m.after_rows(table, &plan);
        let mut steps = match &table.identity {
            Some(column) => identity_steps((target, dialect, schema), &table.name, column).await?,
            None => Vec::new(),
        };
        steps.append(&mut m.now);
        steps.append(&mut m.defaults);
        apply_all(target, steps, &mut m.skipped).await;
    }
    let mut skipped = m.skipped;
    apply_all(target, m.end, &mut skipped).await;
    if objects {
        copy_objects(source, (target, dialect, schema), &m.names, &mut skipped).await?;
    }
    Ok(Migrated { rows, skipped })
}

/// The kinds of object [`copy_objects`] copies, in this order.
const OBJECT_KINDS: [ObjectKind; 5] = [
    ObjectKind::Function,
    ObjectKind::Procedure,
    ObjectKind::View,
    ObjectKind::MaterializedView,
    ObjectKind::Trigger,
];

/// Copies the views, routines and triggers (of the copied `tables`) of the source schema, from
/// their DDL: only between databases of one kind, which read each other's. One that fails is
/// tried again after the others, which it may need.
async fn copy_objects(
    (source, from, source_schema): End<'_>,
    (target, to, schema): End<'_>,
    tables: &HashSet<&str>,
    skipped: &mut Vec<String>,
) -> Result<(), ImportError> {
    let objects: Vec<_> = source
        .objects(source_schema)
        .await?
        .into_iter()
        .filter(|o| OBJECT_KINDS.contains(&o.kind))
        .filter(|o| {
            o.kind != ObjectKind::Trigger || o.parent.as_deref().is_some_and(|p| tables.contains(p))
        })
        .collect();
    if objects.is_empty() {
        return Ok(());
    }
    if from != to || !source.capabilities().object_ddl {
        let names: Vec<&str> = objects.iter().map(|o| o.name.as_str()).collect();
        skipped.push(format!(
            "the views and routines of {} are not copied to {}: {}",
            database(from),
            database(to),
            names.join(", ")
        ));
        return Ok(());
    }
    let mut pending = Vec::new();
    for kind in OBJECT_KINDS {
        for o in objects.iter().filter(|o| o.kind == kind) {
            let object = ObjectRef {
                schema: source_schema,
                name: &o.name,
                kind: o.kind,
                parent: o.parent.as_deref(),
            };
            match source.object_ddl(&object).await {
                Ok(ddl) => {
                    let ddl = requalify(&without_definer(&ddl), from, source_schema, schema);
                    pending.push((o.name.clone(), ddl));
                }
                Err(e) => skipped.push(format!("{}: {e}", o.name)),
            }
        }
    }
    loop {
        let tried = pending.len();
        let mut failed = Vec::new();
        for (name, ddl) in std::mem::take(&mut pending) {
            if let Err(e) = apply(target, &ddl).await {
                failed.push((name, ddl, e));
            }
        }
        if failed.is_empty() {
            return Ok(());
        }
        if failed.len() == tried {
            // Nothing more went through: these fail on their own.
            skipped.extend(failed.into_iter().map(|(n, _, e)| format!("{n}: {e}")));
            return Ok(());
        }
        pending = failed.into_iter().map(|(n, d, _)| (n, d)).collect();
    }
}

/// MySQL's DDL without its `DEFINER=user@host`: the copy belongs to whoever makes it, as
/// naming another user takes a privilege.
fn without_definer(ddl: &str) -> String {
    let Some(at) = ddl.find("DEFINER=") else {
        return ddl.to_owned();
    };
    let rest = &ddl[at..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    format!("{}{}", &ddl[..at], rest[end..].trim_start())
}

/// `sql` of the source with its quoted names quoted as the target quotes them: `[a]` and
/// `` `a` `` become `"a"` in PostgreSQL.
fn requote(sql: &str, (source, target): (Dialect, Dialect)) -> String {
    if source == target {
        return sql.to_owned();
    }
    let mut out = String::with_capacity(sql.len());
    let mut copied = 0;
    for t in Lexer::new(sql, source).filter(|t| t.kind == TokenKind::QuotedIdent) {
        let text = t.text(sql);
        out.push_str(&sql[copied..t.start]);
        out.push_str(&target.quote_ident(&text[1..text.len() - 1]));
        copied = t.end;
    }
    out.push_str(&sql[copied..]);
    out
}

/// `ddl` with the names it qualifies with schema `from` qualified with `to`.
fn requalify(ddl: &str, dialect: Dialect, from: &str, to: &str) -> String {
    if from == to {
        return ddl.to_owned();
    }
    let tokens: Vec<_> = Lexer::new(ddl, dialect).collect();
    let mut out = String::with_capacity(ddl.len());
    let mut copied = 0;
    for (i, t) in tokens.iter().enumerate() {
        let text = t.text(ddl);
        let names_schema = match t.kind {
            TokenKind::Word => text.eq_ignore_ascii_case(from),
            TokenKind::QuotedIdent => text.len() >= 2 && &text[1..text.len() - 1] == from,
            _ => false,
        };
        if names_schema && tokens.get(i + 1).is_some_and(|n| n.text(ddl) == ".") {
            out.push_str(&ddl[copied..t.start]);
            out.push_str(&dialect.quote_ident(to));
            copied = t.end;
        }
    }
    out.push_str(&ddl[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_keys_point_at_the_target_schema() {
        let fk = ForeignKeyMeta {
            name: "orders_customer".to_owned(),
            columns: vec!["customer_id".to_owned()],
            ref_schema: "demo".to_owned(),
            ref_table: "Customers".to_owned(),
            ref_columns: vec!["id".to_owned()],
        };
        assert_eq!(
            foreign_key_sql(Dialect::Postgres, "copy", "orders", &fk),
            r#"ALTER TABLE copy.orders ADD CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES copy."Customers" (id)"#
        );
    }

    #[test]
    fn defaults_that_travel() {
        let postgres = (Dialect::Postgres, Dialect::MySql);
        assert_eq!(
            portable_default("'new'::character varying", postgres, "varchar(10)").as_deref(),
            Some("'new'")
        );
        assert_eq!(
            portable_default("(0)::numeric", postgres, "numeric").as_deref(),
            Some("0")
        );
        assert_eq!(
            portable_default("now()", postgres, "timestamp").as_deref(),
            Some("CURRENT_TIMESTAMP")
        );
        assert_eq!(
            portable_default("nextval('s'::regclass)", postgres, "int"),
            None
        );
        let sql_server = (Dialect::SqlServer, Dialect::Postgres);
        assert_eq!(
            portable_default("((-1.5))", sql_server, "decimal").as_deref(),
            Some("-1.5")
        );
        assert_eq!(
            portable_default("(N'it''s')", sql_server, "nvarchar").as_deref(),
            Some("'it''s'")
        );
        assert_eq!(
            portable_default("(getdate())", sql_server, "datetime").as_deref(),
            Some("CURRENT_TIMESTAMP")
        );
        let mysql = (Dialect::MySql, Dialect::SqlServer);
        assert_eq!(
            portable_default("open", mysql, "varchar(10)").as_deref(),
            Some("'open'")
        );
        assert_eq!(
            portable_default("1", mysql, "tinyint(1)").as_deref(),
            Some("1")
        );
        assert_eq!(
            portable_default("true", (Dialect::Postgres, Dialect::SqlServer), "boolean").as_deref(),
            Some("1")
        );
        assert_eq!(
            portable_default(
                "CURRENT_DATE",
                (Dialect::Postgres, Dialect::SqlServer),
                "date"
            ),
            None
        );
        assert_eq!(portable_default("lower('A')", postgres, "text"), None);
    }

    #[test]
    fn view_definitions_move_to_the_target_schema() {
        let ddl = r#"CREATE VIEW demo.v AS SELECT d.x, "demo".t.y FROM demo.t d JOIN other.u ON 'demo.' = demo"#;
        assert_eq!(
            requalify(ddl, Dialect::Postgres, "demo", "copy"),
            "CREATE VIEW copy.v AS SELECT d.x, copy.t.y FROM copy.t d JOIN other.u ON 'demo.' = demo"
        );
    }

    fn table(name: &str, refs: &[&str]) -> Table {
        Table {
            name: name.to_owned(),
            details: TableDetails {
                foreign_keys: refs
                    .iter()
                    .map(|r| ForeignKeyMeta {
                        name: format!("{name}_{r}"),
                        columns: vec!["k".to_owned()],
                        ref_schema: String::new(),
                        ref_table: (*r).to_owned(),
                        ref_columns: vec!["id".to_owned()],
                    })
                    .collect(),
                ..TableDetails::default()
            },
            fields: std::sync::Arc::new(arrow_schema::Schema::empty()),
            identity: None,
        }
    }

    #[test]
    fn referenced_tables_come_first() {
        let tables = vec![
            table("lines", &["orders", "items"]),
            table("orders", &["customers", "orders"]),
            table("customers", &["elsewhere"]),
            table("items", &[]),
            // A cycle: a goes first anyway.
            table("a", &["b"]),
            table("b", &["a"]),
        ];
        let order: Vec<_> = ordered(tables).into_iter().map(|t| t.name).collect();
        assert_eq!(order, ["customers", "orders", "items", "lines", "a", "b"]);
    }
}
