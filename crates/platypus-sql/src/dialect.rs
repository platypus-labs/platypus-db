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
}

impl From<DriverKind> for Dialect {
    fn from(kind: DriverKind) -> Self {
        match kind {
            DriverKind::Postgres => Self::Postgres,
            DriverKind::MySql => Self::MySql,
            DriverKind::Sqlite => Self::Sqlite,
        }
    }
}

/// Words that cannot be bare identifiers in at least one supported dialect.
const RESERVED: &[&str] = &[
    "ALL",
    "AND",
    "ANY",
    "ARRAY",
    "AS",
    "ASC",
    "BETWEEN",
    "BOTH",
    "BY",
    "CASE",
    "CAST",
    "CHECK",
    "COLLATE",
    "COLUMN",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_USER",
    "DEFAULT",
    "DELETE",
    "DESC",
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
    "FROM",
    "FULL",
    "GRANT",
    "GROUP",
    "HAVING",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LEADING",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NATURAL",
    "NOT",
    "NULL",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "PRIMARY",
    "REFERENCES",
    "RIGHT",
    "ROW",
    "ROWS",
    "SELECT",
    "SET",
    "SOME",
    "TABLE",
    "THEN",
    "TO",
    "TRAILING",
    "TRUE",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USER",
    "USING",
    "VALUES",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
];

fn is_reserved(name: &str) -> bool {
    RESERVED.iter().any(|w| w.eq_ignore_ascii_case(name))
}

impl Dialect {
    /// Quotes an identifier only when the dialect requires it: PostgreSQL folds bare names
    /// to lower case, so any upper-case letter needs quotes there.
    #[must_use]
    pub fn quote_ident(self, name: &str) -> Cow<'_, str> {
        let mut chars = name.chars();
        let first_ok = chars.next().is_some_and(|c| match self {
            Self::Postgres => c.is_ascii_lowercase() || c == '_',
            Self::MySql | Self::Sqlite => c.is_ascii_alphabetic() || c == '_',
        });
        let rest_ok = chars.all(|c| match self {
            Self::Postgres => c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$',
            Self::MySql => c.is_ascii_alphanumeric() || c == '_' || c == '$',
            Self::Sqlite => c.is_ascii_alphanumeric() || c == '_',
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
            Self::Postgres | Self::Sqlite => '"',
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
        }
    }

    /// Whether `SET column = DEFAULT` works; SQLite needs the default expression instead.
    #[must_use]
    pub const fn has_default_keyword(self) -> bool {
        !matches!(self, Self::Sqlite)
    }

    /// An `INSERT` of a row where every column takes its default.
    #[must_use]
    pub fn insert_defaults(self, table: &str) -> String {
        match self {
            Self::MySql => format!("INSERT INTO {table} () VALUES ()"),
            Self::Postgres | Self::Sqlite => format!("INSERT INTO {table} DEFAULT VALUES"),
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
        }
    }

    /// The clause that caps a query's rows, appended after `ORDER BY`.
    #[must_use]
    pub fn limit_clause(self, limit: u64) -> String {
        format!("LIMIT {limit}")
    }

    /// The prefix that turns a statement into a plan request whose output is JSON, or
    /// `None` when the dialect has no JSON plan (SQLite's `EXPLAIN QUERY PLAN` is a table).
    #[must_use]
    pub const fn explain_json_prefix(self, analyze: bool) -> Option<&'static str> {
        match (self, analyze) {
            (Self::Postgres, false) => Some("EXPLAIN (FORMAT JSON)"),
            (Self::Postgres, true) => Some("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)"),
            (Self::MySql, false) => Some("EXPLAIN FORMAT=JSON"),
            (Self::MySql, true) => Some("EXPLAIN ANALYZE FORMAT=JSON"),
            (Self::Sqlite, _) => None,
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
    fn explain_prefixes() {
        assert!(Dialect::Postgres.explain_json_prefix(true).is_some());
        assert_eq!(Dialect::Sqlite.explain_json_prefix(false), None);
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
        assert_eq!(
            Dialect::MySql.insert_defaults("t"),
            "INSERT INTO t () VALUES ()"
        );
    }
}
