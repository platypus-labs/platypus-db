//! What differs between SQL dialects when generating SQL: quoting of identifiers and
//! literals, boolean and binary values, defaults, row limits, `EXPLAIN`. Features that build SQL (data editing, DDL, import,
//! the data viewer) go through these methods instead of matching on the driver.
//!
//! `apps/desktop/src/sqlNames.ts` mirrors [`Dialect::quote_ident`] for the UI; keep the two
//! in sync.

use std::borrow::Cow;

use platypus_core::DriverKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dialect {
    Postgres,
    MySql,
    Sqlite,
    /// PostgreSQL-like syntax; bare names keep their case and match it loosely, like SQLite.
    DuckDb,
}

impl From<DriverKind> for Dialect {
    fn from(kind: DriverKind) -> Self {
        match kind {
            DriverKind::Postgres => Self::Postgres,
            DriverKind::MySql => Self::MySql,
            DriverKind::Sqlite => Self::Sqlite,
            DriverKind::DuckDb => Self::DuckDb,
        }
    }
}

/// Words that cannot be bare identifiers in at least one supported dialect.
const RESERVED: &[&str] = &[
    "ALL",
    "ANALYSE",
    "ANALYZE",
    "AND",
    "ANTI",
    "ANY",
    "ARRAY",
    "AS",
    "ASC",
    "ASOF",
    "ASYMMETRIC",
    "AT",
    "AUTHORIZATION",
    "BETWEEN",
    "BINARY",
    "BOTH",
    "BY",
    "CASE",
    "CAST",
    "CHECK",
    "COLLATE",
    "COLLATION",
    "COLUMN",
    "COLUMNS",
    "CONCURRENTLY",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_USER",
    "DEFAULT",
    "DEFERRABLE",
    "DELETE",
    "DESC",
    "DESCRIBE",
    "DISTINCT",
    "DO",
    "DROP",
    "ELSE",
    "END",
    "EXCEPT",
    "EXISTS",
    "FALSE",
    "FETCH",
    "FOR",
    "FOREIGN",
    "FREEZE",
    "FROM",
    "FULL",
    "GLOB",
    "GRANT",
    "GROUP",
    "HAVING",
    "ILIKE",
    "IN",
    "INDEX",
    "INITIALLY",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "ISNULL",
    "JOIN",
    "KEY",
    "LAMBDA",
    "LATERAL",
    "LEADING",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NATURAL",
    "NOT",
    "NOTNULL",
    "NULL",
    "OFFSET",
    "ON",
    "ONLY",
    "OR",
    "ORDER",
    "OUTER",
    "OVERLAPS",
    "PIVOT",
    "PIVOT_LONGER",
    "PIVOT_WIDER",
    "PLACING",
    "POSITIONAL",
    "PRIMARY",
    "QUALIFY",
    "REFERENCES",
    "RETURNING",
    "RIGHT",
    "ROW",
    "ROWS",
    "SELECT",
    "SEMI",
    "SET",
    "SHOW",
    "SIMILAR",
    "SOME",
    "SUMMARIZE",
    "SYMMETRIC",
    "TABLE",
    "TABLESAMPLE",
    "THEN",
    "TO",
    "TRAILING",
    "TRUE",
    "UNION",
    "UNIQUE",
    "UNPACK",
    "UNPIVOT",
    "UPDATE",
    "USER",
    "USING",
    "VALUES",
    "VARIADIC",
    "VERBOSE",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
];

fn is_reserved(name: &str) -> bool {
    RESERVED.iter().any(|w| w.eq_ignore_ascii_case(name))
}

/// The `LIKE` pattern, with `!` as the escape character, of the texts containing `needle`.
#[must_use]
pub fn contains_pattern(needle: &str) -> String {
    let mut pattern = String::with_capacity(needle.len() + 2);
    pattern.push('%');
    for c in needle.chars() {
        if matches!(c, '%' | '_' | '!') {
            pattern.push('!');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

impl Dialect {
    /// Quotes an identifier only when the dialect requires it: PostgreSQL folds bare names
    /// to lower case, so any upper-case letter needs quotes there.
    #[must_use]
    pub fn quote_ident(self, name: &str) -> Cow<'_, str> {
        let mut chars = name.chars();
        let first_ok = chars.next().is_some_and(|c| match self {
            Self::Postgres => c.is_ascii_lowercase() || c == '_',
            Self::MySql | Self::Sqlite | Self::DuckDb => c.is_ascii_alphabetic() || c == '_',
        });
        let rest_ok = chars.all(|c| match self {
            Self::Postgres => c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$',
            Self::MySql => c.is_ascii_alphanumeric() || c == '_' || c == '$',
            Self::Sqlite | Self::DuckDb => c.is_ascii_alphanumeric() || c == '_',
        });
        if first_ok && rest_ok && !is_reserved(name) {
            return Cow::Borrowed(name);
        }
        let quote = self.ident_quote();
        let escaped = name.replace(quote, &format!("{quote}{quote}"));
        Cow::Owned(format!("{quote}{escaped}{quote}"))
    }

    /// The character that delimits quoted identifiers.
    #[must_use]
    pub const fn ident_quote(self) -> char {
        match self {
            Self::MySql => '`',
            Self::Postgres | Self::Sqlite | Self::DuckDb => '"',
        }
    }

    /// `schema.name`, or just `name` when `schema` is `None` (the session's current one).
    #[must_use]
    pub fn qualified(self, schema: Option<&str>, name: &str) -> String {
        match schema {
            Some(schema) => format!("{}.{}", self.quote_ident(schema), self.quote_ident(name)),
            None => self.quote_ident(name).into_owned(),
        }
    }

    /// A string literal. MySQL treats backslashes as escapes by default
    /// (`NO_BACKSLASH_ESCAPES` off), so they are doubled there.
    #[must_use]
    pub fn quote_literal(self, value: &str) -> String {
        let mut out = String::with_capacity(value.len() + 2);
        out.push('\'');
        for c in value.chars() {
            match c {
                '\'' => out.push_str("''"),
                '\\' if self == Self::MySql => out.push_str("\\\\"),
                c => out.push(c),
            }
        }
        out.push('\'');
        out
    }

    /// A boolean literal; SQLite has no boolean type and stores 1 and 0.
    #[must_use]
    pub const fn bool_literal(self, value: bool) -> &'static str {
        match (self, value) {
            (Self::Sqlite, true) => "1",
            (Self::Sqlite, false) => "0",
            (_, true) => "TRUE",
            (_, false) => "FALSE",
        }
    }

    /// A binary literal from lower-case hex digits. PostgreSQL's `decode` does not depend on
    /// `standard_conforming_strings`, unlike `'\x…'`.
    #[must_use]
    pub fn binary_literal(self, hex: &str) -> String {
        match self {
            Self::Postgres => format!("decode('{hex}', 'hex')"),
            Self::MySql | Self::Sqlite => format!("X'{hex}'"),
            Self::DuckDb => format!("from_hex('{hex}')"),
        }
    }

    /// Whether `SET column = DEFAULT` works; SQLite needs the default expression instead.
    #[must_use]
    pub const fn has_default_keyword(self) -> bool {
        !matches!(self, Self::Sqlite)
    }

    /// Whether `SAVEPOINT` works inside a transaction; DuckDB has none.
    #[must_use]
    pub const fn has_savepoints(self) -> bool {
        !matches!(self, Self::DuckDb)
    }

    /// An `INSERT` of a row where every column takes its default.
    #[must_use]
    pub fn insert_defaults(self, table: &str) -> String {
        match self {
            Self::MySql => format!("INSERT INTO {table} () VALUES ()"),
            Self::Postgres | Self::Sqlite | Self::DuckDb => {
                format!("INSERT INTO {table} DEFAULT VALUES")
            }
        }
    }

    /// A query for about how many rows a table has, read from the statistics where the
    /// database keeps them (SQLite counts them). Its value is NULL when unknown.
    #[must_use]
    pub fn row_estimate_sql(self, schema: Option<&str>, table: &str) -> String {
        match self {
            Self::Postgres => format!(
                "SELECT CASE WHEN c.reltuples < 0 THEN NULL ELSE c.reltuples::bigint END                  FROM pg_class c WHERE c.oid = to_regclass({})",
                self.quote_literal(&self.qualified(schema, table))
            ),
            Self::MySql => format!(
                "SELECT TABLE_ROWS FROM information_schema.TABLES                  WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {}",
                schema.map_or_else(|| "DATABASE()".to_owned(), |s| self.quote_literal(s)),
                self.quote_literal(table)
            ),
            Self::Sqlite => format!("SELECT COUNT(*) FROM {}", self.qualified(schema, table)),
            Self::DuckDb => format!(
                "SELECT estimated_size FROM duckdb_tables() \
                 WHERE database_name = current_database() AND schema_name = {} AND table_name = {}",
                schema.map_or_else(|| "current_schema()".to_owned(), |s| self.quote_literal(s)),
                self.quote_literal(table)
            ),
        }
    }

    /// The clause that caps a query's rows, appended after `ORDER BY`.
    #[must_use]
    pub fn limit_clause(self, limit: u64) -> String {
        format!("LIMIT {limit}")
    }

    /// A condition true when the text `expr` contains `needle`, ignoring case. `%` and `_`
    /// in the needle match themselves.
    #[must_use]
    pub fn contains(self, expr: &str, needle: &str) -> String {
        self.contains_quoted(expr, &self.quote_literal(&contains_pattern(needle)))
    }

    /// [`Dialect::contains`] with the [`contains_pattern`] already quoted, for callers
    /// that quote literals their own way (MySQL's `NO_BACKSLASH_ESCAPES`).
    #[must_use]
    pub fn contains_quoted(self, expr: &str, pattern: &str) -> String {
        match self {
            Self::Postgres | Self::DuckDb => format!("{expr} ILIKE {pattern} ESCAPE '!'"),
            // A binary collation would make LIKE case-sensitive.
            Self::MySql => format!("LOWER({expr}) LIKE LOWER({pattern}) ESCAPE '!'"),
            Self::Sqlite => format!("{expr} LIKE {pattern} ESCAPE '!'"),
        }
    }

    /// A condition true for the rows where any of `columns`, read as text, contains `needle`.
    #[must_use]
    pub fn any_column_contains(self, columns: &[&str], needle: &str) -> String {
        let text = match self {
            Self::Postgres | Self::Sqlite | Self::DuckDb => "TEXT",
            Self::MySql => "CHAR",
        };
        columns
            .iter()
            .map(|c| self.contains(&format!("CAST({} AS {text})", self.quote_ident(c)), needle))
            .collect::<Vec<_>>()
            .join(" OR ")
    }

    /// The prefix that turns a statement into a plan request whose output is JSON, or
    /// `None` when the dialect has no JSON plan (SQLite's `EXPLAIN QUERY PLAN` is a table;
    /// DuckDB's JSON plan is not PostgreSQL's).
    #[must_use]
    pub const fn explain_json_prefix(self, analyze: bool) -> Option<&'static str> {
        match (self, analyze) {
            (Self::Postgres, false) => Some("EXPLAIN (FORMAT JSON)"),
            (Self::Postgres, true) => Some("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)"),
            (Self::MySql, false) => Some("EXPLAIN FORMAT=JSON"),
            (Self::MySql, true) => Some("EXPLAIN ANALYZE FORMAT=JSON"),
            (Self::Sqlite | Self::DuckDb, _) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_identifiers_only_when_needed() {
        let pg = Dialect::Postgres;
        assert_eq!(pg.quote_ident("orders"), "orders");
        assert_eq!(pg.quote_ident("Orders"), "\"Orders\"");
        assert_eq!(pg.quote_ident("order"), "\"order\"");
        assert_eq!(pg.quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(pg.quote_ident("1st"), "\"1st\"");
        assert_eq!(pg.quote_ident(""), "\"\"");
        assert_eq!(Dialect::MySql.quote_ident("Orders"), "Orders");
        assert_eq!(Dialect::MySql.quote_ident("my table"), "`my table`");
        assert_eq!(Dialect::MySql.quote_ident("a`b"), "`a``b`");
        assert_eq!(Dialect::Sqlite.quote_ident("Select"), "\"Select\"");
        assert_eq!(Dialect::Sqlite.quote_ident("a$b"), "\"a$b\"");
        let duck = Dialect::DuckDb;
        assert_eq!(duck.quote_ident("Orders"), "Orders");
        assert_eq!(duck.quote_ident("qualify"), "\"qualify\"");
        assert_eq!(duck.quote_ident("a b"), "\"a b\"");
    }

    #[test]
    fn qualifies_names() {
        assert_eq!(
            Dialect::Postgres.qualified(Some("demo"), "Orders"),
            "demo.\"Orders\""
        );
        assert_eq!(Dialect::MySql.qualified(None, "t"), "t");
    }

    #[test]
    fn quotes_literals() {
        assert_eq!(Dialect::Postgres.quote_literal("it's"), "'it''s'");
        assert_eq!(Dialect::Postgres.quote_literal("a\\b"), "'a\\b'");
        assert_eq!(Dialect::MySql.quote_literal("a\\'b"), "'a\\\\''b'");
        assert_eq!(Dialect::Sqlite.quote_literal("🦆"), "'🦆'");
    }

    #[test]
    fn contains_escapes_wildcards() {
        assert_eq!(
            Dialect::Postgres.contains("name", "a_b%!'"),
            "name ILIKE '%a!_b!%!!''%' ESCAPE '!'"
        );
        assert_eq!(
            Dialect::MySql.contains("c", "X"),
            "LOWER(c) LIKE LOWER('%X%') ESCAPE '!'"
        );
        assert_eq!(
            Dialect::Sqlite.any_column_contains(&["a", "Order"], "x"),
            "CAST(a AS TEXT) LIKE '%x%' ESCAPE '!' OR CAST(\"Order\" AS TEXT) LIKE '%x%' ESCAPE '!'"
        );
        assert!(
            Dialect::MySql
                .any_column_contains(&["a"], "x")
                .contains("CAST(a AS CHAR)")
        );
        assert_eq!(
            Dialect::DuckDb.any_column_contains(&["Total"], "x"),
            "CAST(Total AS TEXT) ILIKE '%x%' ESCAPE '!'"
        );
    }

    #[test]
    fn explain_prefixes() {
        assert!(Dialect::Postgres.explain_json_prefix(true).is_some());
        assert_eq!(Dialect::Sqlite.explain_json_prefix(false), None);
        assert_eq!(Dialect::DuckDb.explain_json_prefix(true), None);
        assert_eq!(Dialect::DuckDb.limit_clause(5), "LIMIT 5");
        assert_eq!(Dialect::MySql.limit_clause(5), "LIMIT 5");
    }

    #[test]
    fn value_literals() {
        assert_eq!(Dialect::Postgres.bool_literal(true), "TRUE");
        assert_eq!(Dialect::Sqlite.bool_literal(false), "0");
        assert_eq!(
            Dialect::Postgres.binary_literal("00ff"),
            "decode('00ff', 'hex')"
        );
        assert_eq!(Dialect::MySql.binary_literal(""), "X''");
        assert!(!Dialect::Sqlite.has_default_keyword());
        assert_eq!(Dialect::DuckDb.binary_literal("00ff"), "from_hex('00ff')");
        assert_eq!(Dialect::DuckDb.bool_literal(true), "TRUE");
        assert!(Dialect::DuckDb.has_default_keyword());
        assert!(!Dialect::DuckDb.has_savepoints() && Dialect::Postgres.has_savepoints());
        assert_eq!(
            Dialect::MySql.insert_defaults("t"),
            "INSERT INTO t () VALUES ()"
        );
        assert_eq!(
            Dialect::DuckDb.insert_defaults("t"),
            "INSERT INTO t DEFAULT VALUES"
        );
    }

    #[test]
    fn estimates_rows_from_the_catalog() {
        let duck = Dialect::DuckDb;
        let sql = duck.row_estimate_sql(Some("main"), "orders");
        assert!(
            sql.starts_with("SELECT estimated_size FROM duckdb_tables()"),
            "{sql}"
        );
        assert!(
            sql.ends_with("schema_name = 'main' AND table_name = 'orders'"),
            "{sql}"
        );
        assert!(
            duck.row_estimate_sql(None, "t")
                .contains("schema_name = current_schema()")
        );
    }
}
