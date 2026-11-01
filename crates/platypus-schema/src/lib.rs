//! The table editor's model: a table as the user edits it ([`TableDesign`]), read from the
//! database with [`load`], and the DDL that turns one design into another ([`script`]),
//! `CREATE TABLE` for a new table. [`apply`] runs that script. A whole schema is read with
//! [`read_tables`] for the ER diagram and [`lint`].

mod catalog;
mod changelog;
mod compare;
mod create;
mod ddl;
mod diff;
mod lint;
mod migrations;
mod model;
mod objects;

use std::fmt::Write;

use platypus_core::{
    Connection, ConstraintKind, ConstraintMeta, Error, ExecOptions, Result, TableDetails,
    TextRowExt, query_text,
};
use platypus_sql::Dialect;
use serde::{Deserialize, Serialize};

pub use changelog::{ChangelogFormat, changelog, flyway_file_name};
pub use compare::{compare, edited_script};
pub use ddl::script;
pub use diff::{ChangeKind, SchemaChange, summary};
pub use lint::{Finding, LintKind, lint};
pub use migrations::{
    Applied, AppliedState, Found, Local, Migration, Status, Tool, ToolStatus, detect, evaluate,
    flyway_checksum, prisma_checksum, status as migration_status,
};
pub use model::{SchemaTable, load_designs, read_tables, tables_of};
pub use objects::{Event, ObjectForm, Param, Timing, create_object};

/// A table as the editor shows it. Each part keeps its name in the database (`original`) so
/// the script can tell a renamed or changed part from a new one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableDesign {
    pub name: String,
    pub columns: Vec<ColumnDesign>,
    /// The name of the primary key constraint, needed to drop it; its columns are the
    /// columns marked `primary_key`.
    #[serde(default)]
    pub primary_key_name: Option<String>,
    pub indexes: Vec<IndexDesign>,
    pub foreign_keys: Vec<ForeignKeyDesign>,
    /// UNIQUE and CHECK constraints.
    pub constraints: Vec<ConstraintDesign>,
    /// The table's comment; `None` for none.
    #[serde(default)]
    pub comment: Option<String>,
    /// What follows the column list of `CREATE TABLE`, kept when the table is created again:
    /// SQLite's `WITHOUT ROWID`, `STRICT`; PostgreSQL's `PARTITION BY RANGE (ts)`;
    /// ClickHouse's `ENGINE = MergeTree ORDER BY .. PARTITION BY .. TTL ..`.
    #[serde(default)]
    pub options: Option<String>,
    /// The `CREATE TRIGGER` statements of the table (SQLite), run again after it is created
    /// again.
    #[serde(default)]
    pub triggers: Vec<String>,
    /// The partitions of a partitioned table (PostgreSQL), created with it.
    #[serde(default)]
    pub partitions: Vec<PartitionDesign>,
    /// The user-defined types its columns use (PostgreSQL's enums, domains, composite and
    /// range types), which a schema comparison creates where they are missing.
    #[serde(default)]
    pub types: Vec<UserType>,
    /// The period of a system-versioned (temporal) table of SQL Server, written in its
    /// column list, and its history table.
    #[serde(default)]
    pub system_time: Option<SystemTime>,
    /// The `PERIOD FOR` clauses of a MariaDB table, written in its column list: the
    /// application-time periods and, for a system-versioned table with row start and end
    /// columns of its own, `SYSTEM_TIME`.
    #[serde(default)]
    pub periods: Vec<PeriodDesign>,
}

/// `PERIOD FOR name (start, end)` of a MariaDB table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodDesign {
    pub name: String,
    pub start: String,
    pub end: String,
}

impl PeriodDesign {
    /// The period of system versioning, whose name is a keyword.
    #[must_use]
    pub fn is_system_time(&self) -> bool {
        self.name.eq_ignore_ascii_case("SYSTEM_TIME")
    }
}

/// `PERIOD FOR SYSTEM_TIME (start, end)` and, when the table is versioned,
/// `SYSTEM_VERSIONING = ON (HISTORY_TABLE = history)`: the history table, in the table's
/// schema, which the database creates and keeps by itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemTime {
    pub start: String,
    pub end: String,
    pub history: Option<String>,
}

/// When a constraint is checked: at the end of each statement, or (PostgreSQL, SQLite) at the
/// end of the transaction when it is deferrable and deferred.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Deferral {
    #[default]
    NotDeferrable,
    /// `DEFERRABLE INITIALLY IMMEDIATE`.
    Immediate,
    /// `DEFERRABLE INITIALLY DEFERRED`.
    Deferred,
}

impl Deferral {
    #[must_use]
    pub fn of(deferrable: bool, deferred: bool) -> Self {
        match (deferrable, deferred) {
            (false, _) => Self::NotDeferrable,
            (true, false) => Self::Immediate,
            (true, true) => Self::Deferred,
        }
    }

    /// The clause that ends the constraint, `None` for one that is not deferrable.
    #[must_use]
    pub fn clause(self) -> Option<&'static str> {
        match self {
            Self::NotDeferrable => None,
            Self::Immediate => Some("DEFERRABLE INITIALLY IMMEDIATE"),
            Self::Deferred => Some("DEFERRABLE INITIALLY DEFERRED"),
        }
    }

    /// The deferral a definition the database printed ends with (`UNIQUE (a) DEFERRABLE
    /// INITIALLY DEFERRED`).
    fn in_definition(definition: &str) -> Self {
        let upper = definition.to_ascii_uppercase();
        let deferrable = upper.contains("DEFERRABLE") && !upper.contains("NOT DEFERRABLE");
        Self::of(deferrable, upper.contains("INITIALLY DEFERRED"))
    }
}

/// A partition of a partitioned table: its name and its bound, `FOR VALUES FROM (..) TO (..)`
/// or `DEFAULT`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionDesign {
    pub name: String,
    pub bound: String,
}

/// A user-defined type, as `CREATE {kind} schema.name {body}` creates it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserType {
    pub schema: String,
    pub name: String,
    /// `TYPE` or `DOMAIN`.
    pub kind: String,
    /// What follows the name: `AS ENUM ('sad', 'ok')`, `AS integer CHECK (VALUE > 0)`.
    pub body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnDesign {
    /// The column's name in the database; `None` for a new column.
    pub original: Option<String>,
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// An SQL expression, e.g. `'open'` or `CURRENT_TIMESTAMP`.
    pub default: Option<String>,
    pub primary_key: bool,
    pub comment: Option<String>,
    /// What the database keeps with the column beyond its type, written again whenever the
    /// column is redefined: MySQL's `auto_increment` and `on update CURRENT_TIMESTAMP`,
    /// SQLite's `AUTOINCREMENT`, the identity of PostgreSQL and Oracle
    /// (`GENERATED ALWAYS AS IDENTITY (START WITH 10 INCREMENT BY 5)`) and SQL Server
    /// (`IDENTITY(10,5)`).
    #[serde(default)]
    pub extra: Option<String>,
    /// The clause that computes a generated column, as the column definition writes it:
    /// `GENERATED ALWAYS AS (a * 2) VIRTUAL`, SQL Server's `AS ([a]*(2)) PERSISTED`. Such a
    /// column has no default.
    #[serde(default)]
    pub generated: Option<String>,
    /// The column's collation where it is not the default one: `NOCASE`, `utf8mb4_bin`.
    #[serde(default)]
    pub collation: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexDesign {
    pub original: Option<String>,
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    /// The index's own `CREATE INDEX` (SQLite), which can say more than its columns (an
    /// expression, a `WHERE`, a collation): run again as it is while the index is unchanged.
    #[serde(default)]
    pub sql: Option<String>,
    /// The key parts as the database writes them, where one says more than its column: an
    /// expression (`lower(name)`), a prefix length (`` `body`(50) ``), an order (`ts DESC`).
    /// Written in place of the columns; empty when the columns say it all, and emptied when
    /// the columns are edited.
    #[serde(default)]
    pub parts: Vec<String>,
    /// The kind of index where it is not the default one: PostgreSQL's access method
    /// (`brin`, `gin`), MySQL's `FULLTEXT` or `SPATIAL`, SQL Server's `CLUSTERED`,
    /// ClickHouse's `TYPE minmax GRANULARITY 1`.
    #[serde(default)]
    pub method: Option<String>,
    /// The columns the index carries without keying on them (`INCLUDE`).
    #[serde(default)]
    pub include: Vec<String>,
    /// The condition of a partial (filtered) index, without `WHERE`.
    #[serde(default)]
    pub predicate: Option<String>,
    /// The word that hides the index from the optimizer, written after its key parts:
    /// MySQL's `INVISIBLE`, MariaDB's `IGNORED`; `None` for a visible index.
    #[serde(default)]
    pub visibility: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignKeyDesign {
    pub original: Option<String>,
    pub name: String,
    pub columns: Vec<String>,
    pub ref_schema: String,
    pub ref_table: String,
    /// Empty when the key references the primary key implicitly (SQLite).
    pub ref_columns: Vec<String>,
    /// `CASCADE`, `SET NULL`, `SET DEFAULT` or `RESTRICT`; `None` for `NO ACTION`.
    #[serde(default)]
    pub on_delete: Option<String>,
    #[serde(default)]
    pub on_update: Option<String>,
    /// `DEFERRABLE ..` (PostgreSQL, SQLite).
    #[serde(default)]
    pub deferral: Deferral,
    /// PostgreSQL's `MATCH FULL` or `MATCH PARTIAL`: `FULL`, `PARTIAL`; `None` for the default
    /// `MATCH SIMPLE`.
    #[serde(default)]
    pub match_type: Option<String>,
    /// A key the database does not check: SQL Server's `NOCHECK`.
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintType {
    #[default]
    Unique,
    Check,
    /// PostgreSQL's `EXCLUDE`, its definition in `check`.
    Exclusion,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstraintDesign {
    pub original: Option<String>,
    pub name: String,
    pub kind: ConstraintType,
    /// The columns of a UNIQUE constraint.
    pub columns: Vec<String>,
    /// The condition of a CHECK, without `CHECK` and its parentheses; what follows `EXCLUDE`
    /// in an exclusion constraint (`USING gist (r WITH &&)`).
    pub check: String,
    /// `DEFERRABLE ..` of a UNIQUE or EXCLUDE (PostgreSQL).
    #[serde(default)]
    pub deferral: Deferral,
    /// PostgreSQL's `UNIQUE NULLS NOT DISTINCT`: one NULL at most.
    #[serde(default)]
    pub nulls_not_distinct: bool,
    /// SQLite's `ON CONFLICT` resolution of a UNIQUE: `REPLACE`, `IGNORE`, `ABORT`, ..
    #[serde(default)]
    pub on_conflict: Option<String>,
    /// A constraint the database does not check: MySQL's `NOT ENFORCED`, SQL Server's
    /// `NOCHECK`.
    #[serde(default)]
    pub disabled: bool,
}

/// The statements that change a table, as [`apply`] runs them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Script {
    /// Run first, outside the transaction (SQLite's `PRAGMA foreign_keys`).
    pub before: Vec<String>,
    pub statements: Vec<String>,
    /// Run last, outside the transaction, even after a failure.
    pub after: Vec<String>,
    /// Whether `statements` run in one transaction; MySQL commits each DDL statement.
    pub transactional: bool,
    /// What the script cannot do or what it costs, for the user to read before running it.
    pub warnings: Vec<String>,
}

impl Script {
    /// The whole script as it runs in an autocommit session of `dialect`, one statement per
    /// line.
    #[must_use]
    pub fn text(&self, dialect: Dialect) -> String {
        let tx = self.transactional && self.statements.len() > 1;
        let mut lines: Vec<&str> = self.before.iter().map(String::as_str).collect();
        if tx {
            // Transact-SQL's `BEGIN` starts a block, not a transaction.
            lines.push(if dialect == Dialect::SqlServer {
                "BEGIN TRANSACTION"
            } else {
                "BEGIN"
            });
        }
        lines.extend(self.statements.iter().map(String::as_str));
        if tx {
            lines.push("COMMIT");
        }
        lines.extend(self.after.iter().map(String::as_str));
        let mut text = String::new();
        for s in lines {
            let _ = writeln!(text, "{s};");
        }
        text
    }
}

/// The design of an existing table, as [`load`] reads it, from its details alone.
#[must_use]
pub fn design_of(dialect: Dialect, table: &str, details: &TableDetails) -> TableDesign {
    // MySQL indexes each foreign key under its name.
    let constraint_named = |name: &str| {
        details.constraints.iter().any(|c| c.name == name)
            || details.foreign_keys.iter().any(|f| f.name == name)
    };
    // SQLite reports every unique index as a constraint; only its automatic indexes are
    // constraints of the table.
    let sqlite_index = |name: &str| dialect == Dialect::Sqlite && !name.starts_with("sqlite_");
    // ClickHouse's driver lists a sorting key longer than the primary key as an index; it is
    // part of the table's engine clause (`options`).
    let sorting_key = |name: &str| dialect == Dialect::ClickHouse && name == "ORDER BY";
    TableDesign {
        name: table.to_owned(),
        columns: details
            .columns
            .iter()
            .map(|c| ColumnDesign {
                original: Some(c.name.clone()),
                name: c.name.clone(),
                data_type: c.data_type.clone(),
                nullable: c.nullable,
                default: c.default.clone(),
                primary_key: c.primary_key,
                comment: c.comment.clone(),
                extra: None,
                generated: None,
                collation: None,
            })
            .collect(),
        primary_key_name: details
            .constraints
            .iter()
            .find(|c| c.kind == ConstraintKind::PrimaryKey)
            .map(|c| c.name.clone()),
        indexes: details
            .indexes
            .iter()
            .filter(|i| {
                !i.primary
                    && !sorting_key(&i.name)
                    && (!constraint_named(&i.name) || sqlite_index(&i.name))
            })
            .map(|i| IndexDesign {
                original: Some(i.name.clone()),
                name: i.name.clone(),
                columns: i.columns.clone(),
                unique: i.unique,
                ..IndexDesign::default()
            })
            .collect(),
        foreign_keys: details
            .foreign_keys
            .iter()
            .map(|f| ForeignKeyDesign {
                original: Some(f.name.clone()),
                name: f.name.clone(),
                columns: f.columns.clone(),
                ref_schema: f.ref_schema.clone(),
                ref_table: f.ref_table.clone(),
                ref_columns: f
                    .ref_columns
                    .iter()
                    .filter(|c| !c.is_empty())
                    .cloned()
                    .collect(),
                ..ForeignKeyDesign::default()
            })
            .collect(),
        constraints: details
            .constraints
            .iter()
            .filter_map(|c| constraint_of(dialect, c, sqlite_index(&c.name)))
            .collect(),
        ..TableDesign::default()
    }
}

/// The UNIQUE, CHECK or EXCLUDE constraint `c` as the design holds it; `None` for a primary
/// key, and for a unique index SQLite lists as a constraint (`index`).
fn constraint_of(dialect: Dialect, c: &ConstraintMeta, index: bool) -> Option<ConstraintDesign> {
    let kind = match c.kind {
        ConstraintKind::Unique if !index => ConstraintType::Unique,
        ConstraintKind::Check => ConstraintType::Check,
        ConstraintKind::Exclusion => ConstraintType::Exclusion,
        ConstraintKind::Unique | ConstraintKind::PrimaryKey => return None,
    };
    // PostgreSQL prints the options of a UNIQUE or EXCLUDE in its definition.
    let printed = dialect == Dialect::Postgres && kind != ConstraintType::Check;
    Some(ConstraintDesign {
        original: Some(c.name.clone()),
        // SQLite names its automatic indexes; the constraint itself has no name.
        name: if c.name.starts_with("sqlite_autoindex_") {
            String::new()
        } else {
            c.name.clone()
        },
        kind,
        columns: if kind == ConstraintType::Unique {
            c.columns.clone()
        } else {
            Vec::new()
        },
        check: match kind {
            ConstraintType::Check => check_condition(&c.definition),
            ConstraintType::Exclusion => exclusion_body(&c.definition),
            ConstraintType::Unique => String::new(),
        },
        deferral: if printed {
            Deferral::in_definition(&c.definition)
        } else {
            Deferral::NotDeferrable
        },
        nulls_not_distinct: printed
            && c.definition
                .to_ascii_uppercase()
                .contains("NULLS NOT DISTINCT"),
        ..ConstraintDesign::default()
    })
}

/// What follows `EXCLUDE` in an exclusion constraint as PostgreSQL prints it, without the
/// deferral it ends with: `EXCLUDE USING gist (r WITH &&) DEFERRABLE` gives
/// `USING gist (r WITH &&)`.
fn exclusion_body(definition: &str) -> String {
    let mut text = definition.trim();
    if text.len() >= 7 && text[..7].eq_ignore_ascii_case("EXCLUDE") {
        text = text[7..].trim_start();
    }
    for suffix in [" INITIALLY DEFERRED", " INITIALLY IMMEDIATE", " DEFERRABLE"] {
        if text.len() >= suffix.len()
            && text[text.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
        {
            text = text[..text.len() - suffix.len()].trim_end();
        }
    }
    text.to_owned()
}

/// The condition of a CHECK as the database prints it: `CHECK ((total >= 0))` gives
/// `(total >= 0)`.
fn check_condition(definition: &str) -> String {
    let mut text = definition.trim();
    if text.len() >= 5 && text[..5].eq_ignore_ascii_case("CHECK") {
        text = text[5..].trim_start();
    }
    match text.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        Some(inner) if balanced(inner) => inner.trim().to_owned(),
        _ => text.to_owned(),
    }
}

/// Whether the parentheses of `text` pair up, ignoring quoted text.
fn balanced(text: &str) -> bool {
    let mut depth = 0i32;
    let mut quote = None;
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// Reads the design of `schema.table`: its details, and what they leave out that the script
/// must not lose (foreign key actions, MySQL's column extras and literal defaults, SQLite's
/// `AUTOINCREMENT`, identity and generated columns, collations, index kinds and expressions,
/// partitioning and engine clauses: see [`catalog`]).
pub async fn load(
    conn: &dyn Connection,
    dialect: Dialect,
    schema: &str,
    table: &str,
) -> Result<TableDesign> {
    let details = conn.table_details(schema, table).await?;
    let mut design = design_of(dialect, table, &details);
    design.comment = conn.object_comment(schema, table).await?;
    key_options(conn, dialect, &mut design, schema, table).await?;
    let (s, t) = (dialect.quote_literal(schema), dialect.quote_literal(table));
    match dialect {
        Dialect::MySql => {
            mysql_columns(conn, &mut design, &s, &t).await?;
            let sql = format!(
                "SHOW CREATE TABLE {}",
                dialect.qualified(Some(schema), table)
            );
            if let Some(create) = query_text(conn, &sql).await?.first().and_then(|r| r.opt(1)) {
                let definition = create::parse(&create, dialect);
                apply_definition(&mut design, &definition);
                mysql_unique_keys(&mut design, &definition);
                catalog::mysql(&mut design, &definition);
            }
        }
        Dialect::Postgres => catalog::postgres(conn, &mut design, schema, table).await,
        Dialect::SqlServer => catalog::sqlserver(conn, &mut design, schema, table).await?,
        Dialect::ClickHouse => catalog::clickhouse(conn, &mut design, schema, table).await?,
        Dialect::Sqlite => sqlite_definitions(conn, &mut design, schema, &t).await?,
        // An identity column reports the sequence Oracle made for it as its default.
        Dialect::Oracle => {
            let sql = format!(
                "SELECT COLUMN_NAME, GENERATION_TYPE FROM ALL_TAB_IDENTITY_COLS
                 WHERE OWNER = {s} AND TABLE_NAME = {t}"
            );
            for row in query_text(conn, &sql).await? {
                if let Some(c) = design.columns.iter_mut().find(|c| c.name == row.text(0)) {
                    c.default = None;
                    c.extra = Some(format!("GENERATED {} AS IDENTITY", row.text(1)));
                }
            }
        }
        Dialect::DuckDb => {
            let sql = format!(
                "SELECT sql FROM duckdb_tables()
                 WHERE database_name = current_database() AND schema_name = {s} AND table_name = {t}"
            );
            if let Some(create) = query_text(conn, &sql).await?.first().and_then(|r| r.opt(0)) {
                apply_definition(&mut design, &create::parse(&create, dialect));
            }
        }
        _ => {}
    }
    Ok(design)
}

/// What the catalogs say of the foreign keys of `design` beyond their columns: their actions,
/// and where the database has them whether they are deferrable and deferred, their match type
/// and whether they are disabled.
async fn key_options(
    conn: &dyn Connection,
    dialect: Dialect,
    design: &mut TableDesign,
    schema: &str,
    table: &str,
) -> Result<()> {
    let (s, t) = (dialect.quote_literal(schema), dialect.quote_literal(table));
    // The name and actions of each key; then whether it is deferrable and deferred, its
    // match type and whether it is disabled, where the database has them.
    let actions = match dialect {
        Dialect::Postgres => Some(format!(
            "SELECT conname, confdeltype, confupdtype, condeferrable, condeferred,
                    confmatchtype::text
             FROM pg_constraint WHERE contype = 'f' AND conrelid = {}::regclass",
            dialect.quote_literal(&dialect.qualified(Some(schema), table))
        )),
        Dialect::MySql => Some(format!(
            // MariaDB reports no action as RESTRICT, which is the same there.
            "SELECT CONSTRAINT_NAME, NULLIF(DELETE_RULE, 'RESTRICT'), NULLIF(UPDATE_RULE, 'RESTRICT')
             FROM information_schema.REFERENTIAL_CONSTRAINTS
             WHERE CONSTRAINT_SCHEMA = {s} AND TABLE_NAME = {t}"
        )),
        Dialect::SqlServer => Some(format!(
            "SELECT name, delete_referential_action_desc, update_referential_action_desc,
                    NULL, NULL, NULL, is_disabled
             FROM sys.foreign_keys WHERE parent_object_id = OBJECT_ID({})",
            dialect.quote_literal(&dialect.qualified(Some(schema), table))
        )),
        // The driver names the keys of a table `fk_<table>_<id>`.
        Dialect::Sqlite => Some(format!(
            "SELECT 'fk_' || {t} || '_' || id, on_delete, on_update
             FROM pragma_foreign_key_list({t}, {s}) GROUP BY id"
        )),
        // Oracle has no `ON UPDATE`.
        Dialect::Oracle => Some(format!(
            "SELECT CONSTRAINT_NAME, DELETE_RULE, NULL FROM ALL_CONSTRAINTS
             WHERE CONSTRAINT_TYPE = 'R' AND OWNER = {s} AND TABLE_NAME = {t}"
        )),
        Dialect::Db2 => Some(format!(
            "SELECT CONSTNAME, DELETERULE, UPDATERULE FROM SYSCAT.REFERENCES
             WHERE TABSCHEMA = {s} AND TABNAME = {t}"
        )),
        // DuckDB has no referential actions, ClickHouse no foreign keys.
        Dialect::DuckDb
        | Dialect::ClickHouse
        | Dialect::Ansi
        | Dialect::Snowflake
        | Dialect::BigQuery
        | Dialect::Databricks => None,
    };
    if let Some(sql) = actions {
        for row in query_text(conn, &sql).await? {
            if let Some(fk) = design
                .foreign_keys
                .iter_mut()
                .find(|f| f.name == row.text(0))
            {
                fk.on_delete = action(&row.text(1));
                fk.on_update = action(&row.text(2));
                fk.deferral = Deferral::of(row.flag(3), row.flag(4));
                fk.match_type = match row.text(5).as_str() {
                    "f" => Some("FULL".to_owned()),
                    "p" => Some("PARTIAL".to_owned()),
                    _ => None,
                };
                fk.disabled = row.flag(6);
            }
        }
    }
    Ok(())
}

/// MySQL: a unique key on a prefix (`` `email`(20) ``), an expression or in descending order
/// becomes a unique index of the design, whose key parts [`catalog::mysql`] then keeps; as a
/// `UNIQUE (email)` constraint it would lose them, and fail on a `TEXT` column. One on whole
/// columns stays a constraint.
fn mysql_unique_keys(design: &mut TableDesign, definition: &create::Definition) {
    for parsed in definition.indexes.iter().filter(|i| i.unique) {
        let Some(at) = design
            .constraints
            .iter()
            .position(|c| c.kind == ConstraintType::Unique && c.name == parsed.name)
        else {
            continue;
        };
        // `SHOW CREATE TABLE` quotes every name.
        let columns = &design.constraints[at].columns;
        let plain = parsed.parts.len() == columns.len()
            && parsed
                .parts
                .iter()
                .zip(columns)
                .all(|(p, c)| *p == Dialect::MySql.quote_ident_always(c));
        if plain {
            continue;
        }
        let key = design.constraints.remove(at);
        design.indexes.push(IndexDesign {
            original: key.original,
            name: key.name,
            columns: key.columns,
            unique: true,
            ..IndexDesign::default()
        });
    }
}

/// What the design keeps of a SQLite table beyond its details, read from the SQL SQLite keeps
/// of it: `AUTOINCREMENT`, generated columns, collations, table options, the statements of
/// its indexes and its triggers.
async fn sqlite_definitions(
    conn: &dyn Connection,
    design: &mut TableDesign,
    schema: &str,
    table: &str,
) -> Result<()> {
    let sql = format!(
        "SELECT type, name, sql FROM {}.sqlite_master
         WHERE tbl_name = {table} AND type IN ('table', 'index', 'trigger') AND sql IS NOT NULL
         ORDER BY type = 'table' DESC, name",
        Dialect::Sqlite.quote_ident(schema)
    );
    for row in query_text(conn, &sql).await? {
        let (kind, name, text) = (row.text(0), row.text(1), row.text(2));
        match kind.as_str() {
            "table" => {
                let definition = create::parse(&text, Dialect::Sqlite);
                apply_definition(design, &definition);
                let mut keys = design.columns.iter_mut().filter(|c| c.primary_key);
                if let (true, Some(key), None) =
                    (definition.autoincrement, keys.next(), keys.next())
                {
                    key.extra = Some("AUTOINCREMENT".to_owned());
                }
            }
            "index" => {
                if let Some(index) = design.indexes.iter_mut().find(|i| i.name == name) {
                    // Its expressions, orders and `WHERE`, for a comparison to write.
                    if let Some(parsed) = create::index(&text, Dialect::Sqlite) {
                        index.parts = catalog::kept_parts(Dialect::Sqlite, index, parsed.parts);
                        index.predicate = parsed.predicate;
                    }
                    index.sql = Some(text);
                }
            }
            _ => design.triggers.push(text),
        }
    }
    Ok(())
}

/// Copies what a parsed `CREATE TABLE` says of its columns and options into `design`.
fn apply_definition(design: &mut TableDesign, definition: &create::Definition) {
    for parsed in &definition.columns {
        let Some(column) = design.columns.iter_mut().find(|c| c.name == parsed.name) else {
            continue;
        };
        if parsed.generated.is_some() {
            // DuckDB reports the expression as the column's default.
            column.default = None;
            column.generated.clone_from(&parsed.generated);
        }
        column.collation.clone_from(&parsed.collation);
    }
    design.options.clone_from(&definition.options);
    design.periods.clone_from(&definition.periods);
    // What the catalogs leave out of the keys and constraints: SQLite's deferral, MATCH and
    // ON CONFLICT, MySQL's NOT ENFORCED. Unnamed there, keys and UNIQUEs are found by their
    // columns.
    let same = |a: &[String], b: &[String]| {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
    };
    for key in &definition.foreign_keys {
        if let Some(fk) = design.foreign_keys.iter_mut().find(|f| {
            same(&f.columns, &key.columns) && f.ref_table.eq_ignore_ascii_case(&key.ref_table)
        }) {
            fk.deferral = key.deferral;
            fk.match_type.clone_from(&key.match_type);
        }
    }
    for unique in &definition.uniques {
        if let Some(c) = design
            .constraints
            .iter_mut()
            .find(|c| c.kind == ConstraintType::Unique && same(&c.columns, &unique.columns))
        {
            c.on_conflict.clone_from(&unique.on_conflict);
        }
    }
    for check in definition.checks.iter().filter(|c| c.not_enforced) {
        if let Some(c) = design
            .constraints
            .iter_mut()
            .find(|c| c.kind == ConstraintType::Check && c.name == check.name)
        {
            c.disabled = true;
        }
    }
}

/// A referential action as the catalogs name it (`c`, `CASCADE`, `SET_NULL`), or `None` for
/// no action.
fn action(text: &str) -> Option<String> {
    let action = match text.trim().to_ascii_uppercase().replace('_', " ").as_str() {
        "C" | "CASCADE" => "CASCADE",
        "N" | "SET NULL" => "SET NULL",
        "D" | "SET DEFAULT" => "SET DEFAULT",
        "R" | "RESTRICT" => "RESTRICT",
        _ => return None,
    };
    Some(action.to_owned())
}

/// `CURRENT_TIMESTAMP` or `NOW()`, which MySQL takes without parentheses.
fn is_now(default: &str) -> bool {
    let upper = default.to_ascii_uppercase();
    upper.starts_with("CURRENT_TIMESTAMP") || upper.starts_with("NOW(")
}

/// A temporal column's `CURRENT_TIMESTAMP` default: an expression, though MySQL 5.7 does not
/// mark it `DEFAULT_GENERATED` as 8.0 does.
fn current_time(default: &str, data_type: &str) -> bool {
    let data_type = data_type.to_ascii_lowercase();
    let temporal = ["datetime", "timestamp"]
        .iter()
        .any(|t| data_type.starts_with(t));
    temporal && is_now(default)
}

/// MySQL prints a literal default without its quotes and keeps `auto_increment` apart from
/// the type; both must be written back when a column is redefined. MariaDB quotes literals.
async fn mysql_columns(
    conn: &dyn Connection,
    design: &mut TableDesign,
    schema: &str,
    table: &str,
) -> Result<()> {
    let sql = format!(
        "SELECT COLUMN_NAME, EXTRA, VERSION() LIKE '%MariaDB%'
         FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = {schema} AND TABLE_NAME = {table}"
    );
    for row in query_text(conn, &sql).await? {
        let Some(column) = design.columns.iter_mut().find(|c| c.name == row.text(0)) else {
            continue;
        };
        let extra = row.text(1);
        let generated = extra.contains("DEFAULT_GENERATED");
        // A generated column's `VIRTUAL GENERATED` is not syntax: its clause comes from
        // SHOW CREATE TABLE below.
        let kept: Vec<&str> = extra
            .split_whitespace()
            .filter(|w| {
                ![
                    "DEFAULT_GENERATED",
                    "GENERATED",
                    "VIRTUAL",
                    "STORED",
                    "PERSISTENT",
                ]
                .iter()
                .any(|g| w.eq_ignore_ascii_case(g))
            })
            .collect();
        column.extra = (!kept.is_empty()).then(|| kept.join(" "));
        let mariadb = row.flag(2);
        column.default = column.default.take().and_then(|d| {
            if mariadb {
                // MariaDB writes no default of a nullable column as `NULL`.
                (d != "NULL").then_some(d)
            } else if generated || current_time(&d, &column.data_type) {
                Some(if is_now(&d) { d } else { format!("({d})") })
            } else if d.parse::<f64>().is_ok() || is_bit_or_hex_literal(&d) {
                Some(d)
            } else {
                Some(Dialect::MySql.quote_string(&d))
            }
        });
    }
    Ok(())
}

/// `b'101'`, `x'1F'` or `0x1F`, which MySQL prints as they are written.
fn is_bit_or_hex_literal(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.starts_with("0x")
        || ((lower.starts_with("b'") || lower.starts_with("x'")) && lower.ends_with('\''))
}

/// Why [`apply`] stopped: the statement that failed and the database's error.
#[derive(Debug)]
pub struct ApplyError {
    pub statement: String,
    pub error: Error,
    /// The statements that ran before it and stay applied: a script that is not
    /// transactional (MySQL, ClickHouse) cannot undo them.
    pub applied: Vec<String>,
}

async fn exec(conn: &dyn Connection, sql: &str) -> Result<()> {
    // Dropping a result stream ends the statement.
    conn.execute(sql, &ExecOptions::default()).await.map(drop)
}

/// Runs `script`: its statements in one transaction where the database can roll DDL back
/// and the session is in autocommit mode; in manual-commit mode they join the session's
/// transaction and stay uncommitted.
pub async fn apply(
    conn: &dyn Connection,
    script: &Script,
) -> std::result::Result<(), Box<ApplyError>> {
    let fail = |statement: &str| {
        let statement = statement.to_owned();
        move |error| {
            Box::new(ApplyError {
                statement,
                error,
                applied: Vec::new(),
            })
        }
    };
    let autocommit = conn.transaction().autocommit;
    // SQLite ignores `PRAGMA foreign_keys` inside a transaction: dropping the old table would
    // then cascade into the tables that reference it.
    if !script.before.is_empty() && !autocommit {
        return Err(fail("PRAGMA foreign_keys = OFF")(Error::Other(
            "the table must be rebuilt outside a transaction: commit or roll back first".to_owned(),
        )));
    }
    for s in &script.before {
        exec(conn, s).await.map_err(fail(s))?;
    }
    let tx = script.transactional && script.statements.len() > 1 && autocommit;
    let mut result = if tx {
        exec(conn, "BEGIN").await.map_err(fail("BEGIN"))
    } else {
        Ok(())
    };
    if result.is_ok() {
        for (i, s) in script.statements.iter().enumerate() {
            if let Err(e) = exec(conn, s).await {
                if tx {
                    let _ = exec(conn, "ROLLBACK").await;
                }
                let mut error = fail(s)(e);
                if !script.transactional {
                    error.applied = script.statements[..i].to_vec();
                }
                result = Err(error);
                break;
            }
        }
    }
    if tx && result.is_ok() {
        result = exec(conn, "COMMIT").await.map_err(fail("COMMIT"));
    }
    for s in &script.after {
        let done = exec(conn, s).await.map_err(fail(s));
        if result.is_ok() {
            result = done;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mysql_unique_key_on_a_prefix_becomes_a_unique_index_with_its_parts() {
        let unique = |name: &str, columns: &[&str]| ConstraintDesign {
            original: Some(name.to_owned()),
            name: name.to_owned(),
            kind: ConstraintType::Unique,
            columns: columns.iter().map(|&c| c.to_owned()).collect(),
            ..ConstraintDesign::default()
        };
        let mut design = TableDesign {
            constraints: vec![
                unique("u_email", &["email"]),
                unique("u_body", &["body", "id"]),
            ],
            ..TableDesign::default()
        };
        let definition = create::parse(
            "CREATE TABLE `t` (\n  `id` int NOT NULL,\n  `email` varchar(90),\n  `body` text,\n  \
             UNIQUE KEY `u_email` (`email`),\n  UNIQUE KEY `u_body` (`body`(20),`id` DESC)\n) ENGINE=InnoDB",
            Dialect::MySql,
        );
        mysql_unique_keys(&mut design, &definition);
        catalog::mysql(&mut design, &definition);
        // On whole columns: still a constraint.
        assert_eq!(design.constraints, [unique("u_email", &["email"])]);
        assert_eq!(
            design.indexes,
            [IndexDesign {
                original: Some("u_body".into()),
                name: "u_body".into(),
                columns: vec!["body".into(), "id".into()],
                unique: true,
                parts: vec!["`body`(20)".into(), "`id` DESC".into()],
                ..IndexDesign::default()
            }]
        );
    }

    #[test]
    fn a_time_columns_current_timestamp_is_an_expression() {
        assert!(current_time("CURRENT_TIMESTAMP", "datetime"));
        assert!(current_time("current_timestamp(6)", "TIMESTAMP(6)"));
        assert!(current_time("now()", "datetime"));
        assert!(
            !current_time("CURRENT_TIMESTAMP", "varchar(20)"),
            "text, as written"
        );
        assert!(!current_time("2026-01-01 00:00:00", "datetime"));
    }

    #[test]
    fn check_conditions_lose_their_keyword_and_outer_parentheses() {
        assert_eq!(check_condition("CHECK ((total >= 0))"), "(total >= 0)");
        assert_eq!(check_condition("check (a > 0)"), "a > 0");
        assert_eq!(
            check_condition("(a > 0) AND (b > 0)"),
            "(a > 0) AND (b > 0)"
        );
        assert_eq!(check_condition("a IN ('(', 'b')"), "a IN ('(', 'b')");
    }

    #[test]
    fn postgres_constraints_keep_the_options_their_definition_prints() {
        let meta = |kind, definition: &str| ConstraintMeta {
            name: "c".into(),
            kind,
            columns: vec!["a1".into(), "a2".into()],
            definition: definition.into(),
        };
        let unique = constraint_of(
            Dialect::Postgres,
            &meta(
                ConstraintKind::Unique,
                "UNIQUE NULLS NOT DISTINCT (a1, a2) DEFERRABLE INITIALLY DEFERRED",
            ),
            false,
        )
        .unwrap();
        assert!(unique.nulls_not_distinct);
        assert_eq!(unique.deferral, Deferral::Deferred);
        let exclusion = constraint_of(
            Dialect::Postgres,
            &meta(
                ConstraintKind::Exclusion,
                "EXCLUDE USING gist (r WITH &&) DEFERRABLE",
            ),
            false,
        )
        .unwrap();
        assert_eq!(
            (
                exclusion.kind,
                exclusion.check.as_str(),
                exclusion.deferral,
                exclusion.columns.len()
            ),
            (
                ConstraintType::Exclusion,
                "USING gist (r WITH &&)",
                Deferral::Immediate,
                0
            )
        );
        // Another database's definition says nothing of them.
        let mysql = constraint_of(
            Dialect::MySql,
            &meta(ConstraintKind::Unique, "DEFERRABLE"),
            false,
        )
        .unwrap();
        assert_eq!(mysql.deferral, Deferral::NotDeferrable);
        assert!(constraint_of(Dialect::Sqlite, &meta(ConstraintKind::Unique, ""), true).is_none());
    }

    #[test]
    fn sqlite_keys_and_uniques_take_what_the_create_table_says() {
        let mut design = TableDesign {
            foreign_keys: vec![ForeignKeyDesign {
                name: "fk_t_0".into(),
                columns: vec!["r".into()],
                ref_table: "f0".into(),
                ..ForeignKeyDesign::default()
            }],
            constraints: vec![ConstraintDesign {
                columns: vec!["U".into()],
                ..ConstraintDesign::default()
            }],
            ..TableDesign::default()
        };
        let definition = create::parse(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, r INT REFERENCES F0(id) DEFERRABLE INITIALLY DEFERRED,
               u TEXT UNIQUE ON CONFLICT REPLACE)",
            Dialect::Sqlite,
        );
        apply_definition(&mut design, &definition);
        assert_eq!(design.foreign_keys[0].deferral, Deferral::Deferred);
        assert_eq!(
            design.constraints[0].on_conflict.as_deref(),
            Some("REPLACE")
        );
    }

    #[test]
    fn bit_and_hex_literals_keep_their_form() {
        assert!(is_bit_or_hex_literal("b'1'"));
        assert!(is_bit_or_hex_literal("0x1F"));
        assert!(!is_bit_or_hex_literal("bob"));
    }

    #[test]
    fn referential_actions_from_every_catalog() {
        assert_eq!(action("c").as_deref(), Some("CASCADE"));
        assert_eq!(action("SET_NULL").as_deref(), Some("SET NULL"));
        assert_eq!(action("NO ACTION"), None);
        assert_eq!(action("a"), None);
    }

    #[test]
    fn scripts_wrap_several_statements_in_a_transaction() {
        let script = Script {
            before: vec!["PRAGMA foreign_keys = OFF".into()],
            statements: vec!["A".into(), "B".into()],
            after: vec!["PRAGMA foreign_keys = ON".into()],
            transactional: true,
            warnings: Vec::new(),
        };
        assert_eq!(
            script.text(Dialect::Sqlite),
            "PRAGMA foreign_keys = OFF;\nBEGIN;\nA;\nB;\nCOMMIT;\nPRAGMA foreign_keys = ON;\n"
        );
        let server = Script {
            before: Vec::new(),
            after: Vec::new(),
            ..script
        };
        assert_eq!(
            server.text(Dialect::SqlServer),
            "BEGIN TRANSACTION;\nA;\nB;\nCOMMIT;\n"
        );
    }
}
