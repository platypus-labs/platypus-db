//! What differs between SQL dialects when generating SQL: quoting of identifiers and
//! literals, boolean and binary values, defaults, row limits. Features that build SQL (data editing, DDL, import,
//! the data viewer) go through these methods instead of matching on the driver.
//!
//! `apps/desktop/src/sqlNames.ts` mirrors [`Dialect::quote_ident`] for the UI; keep the two
//! in sync.

use std::borrow::Cow;

use platypus_core::DriverKind;
use platypus_core::arrow_schema::DataType;

use crate::{Lexer, Token, TokenKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dialect {
    Postgres,
    MySql,
    Sqlite,
    /// PostgreSQL-like syntax; bare names keep their case and match it loosely, like SQLite.
    DuckDb,
    /// Transact-SQL: Microsoft SQL Server and Azure SQL.
    SqlServer,
    /// Case-sensitive names; strings and quoted names take backslash escapes.
    ClickHouse,
    /// Standard SQL with `"…"` names: Snowflake, Trino and generic ADBC drivers.
    Ansi,
    /// `GoogleSQL`: `` `…` `` names, `"…"` strings, backslash escapes in both.
    BigQuery,
    /// Spark SQL on Databricks: `` `…` `` names (a backtick doubled inside), `"…"` strings
    /// with backslash escapes.
    Databricks,
}

impl From<DriverKind> for Dialect {
    fn from(kind: DriverKind) -> Self {
        match kind {
            // MongoDB speaks no SQL: the app never builds SQL for it (the editor, data
            // viewer and navigator write mongosh instead), so any dialect would do here.
            DriverKind::Postgres | DriverKind::Mongo => Self::Postgres,
            DriverKind::Snowflake | DriverKind::Trino | DriverKind::Adbc => Self::Ansi,
            DriverKind::BigQuery => Self::BigQuery,
            DriverKind::Databricks => Self::Databricks,
            DriverKind::MySql => Self::MySql,
            DriverKind::Sqlite | DriverKind::Libsql => Self::Sqlite,
            DriverKind::DuckDb => Self::DuckDb,
            DriverKind::SqlServer => Self::SqlServer,
            DriverKind::ClickHouse => Self::ClickHouse,
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

/// Further words that Transact-SQL reserves.
const TSQL_RESERVED: &[&str] = &[
    "ADD",
    "BACKUP",
    "BEGIN",
    "BREAK",
    "BROWSE",
    "BULK",
    "CASCADE",
    "CHECKPOINT",
    "CLOSE",
    "CLUSTERED",
    "COALESCE",
    "COMMIT",
    "COMPUTE",
    "CONTAINS",
    "CONTAINSTABLE",
    "CONTINUE",
    "CONVERT",
    "CURSOR",
    "DATABASE",
    "DBCC",
    "DEALLOCATE",
    "DECLARE",
    "DENY",
    "DISK",
    "DISTRIBUTED",
    "DOUBLE",
    "DUMP",
    "ERRLVL",
    "ESCAPE",
    "EXEC",
    "EXECUTE",
    "EXIT",
    "EXTERNAL",
    "FILE",
    "FILLFACTOR",
    "FREETEXT",
    "FREETEXTTABLE",
    "FUNCTION",
    "GOTO",
    "HOLDLOCK",
    "IDENTITY",
    "IDENTITYCOL",
    "IDENTITY_INSERT",
    "IF",
    "KILL",
    "LINENO",
    "LOAD",
    "MERGE",
    "NATIONAL",
    "NOCHECK",
    "NONCLUSTERED",
    "NULLIF",
    "OF",
    "OFF",
    "OFFSETS",
    "OPEN",
    "OPENDATASOURCE",
    "OPENQUERY",
    "OPENROWSET",
    "OPENXML",
    "OPTION",
    "OVER",
    "PERCENT",
    "PIVOT",
    "PLAN",
    "PRECISION",
    "PRINT",
    "PROC",
    "PROCEDURE",
    "PUBLIC",
    "RAISERROR",
    "READ",
    "READTEXT",
    "RECONFIGURE",
    "REPLICATION",
    "RESTORE",
    "RESTRICT",
    "RETURN",
    "REVERT",
    "REVOKE",
    "ROLLBACK",
    "ROWCOUNT",
    "ROWGUIDCOL",
    "RULE",
    "SAVE",
    "SCHEMA",
    "SECURITYAUDIT",
    "SEMANTICKEYPHRASETABLE",
    "SESSION_USER",
    "SETUSER",
    "SHUTDOWN",
    "STATISTICS",
    "SYSTEM_USER",
    "TABLESAMPLE",
    "TEXTSIZE",
    "TOP",
    "TRAN",
    "TRANSACTION",
    "TRIGGER",
    "TRUNCATE",
    "TRY_CONVERT",
    "TSEQUAL",
    "UNPIVOT",
    "UPDATETEXT",
    "USE",
    "VARYING",
    "VIEW",
    "WAITFOR",
    "WHILE",
    "WITHIN",
    "WRITETEXT",
];

fn is_reserved(name: &str, dialect: Dialect) -> bool {
    // Both lists are sorted; the `sorted_reserved_words` test keeps them so.
    let upper = name.to_ascii_uppercase();
    let found = |list: &[&str]| list.binary_search(&upper.as_str()).is_ok();
    found(RESERVED) || (dialect == Dialect::SqlServer && found(TSQL_RESERVED))
}

/// The `LIKE` pattern, with `!` as the escape character, of the texts containing `needle`.
/// `[` starts a character class in Transact-SQL patterns, so it is escaped too.
#[must_use]
pub fn contains_pattern(needle: &str) -> String {
    let mut pattern = String::with_capacity(needle.len() + 2);
    pattern.push('%');
    for c in needle.chars() {
        if matches!(c, '%' | '_' | '!' | '[') {
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
            // Snowflake folds bare names to upper case, Trino to lower case.
            Self::Ansi => c.is_ascii_uppercase() || c == '_',
            Self::MySql
            | Self::Sqlite
            | Self::DuckDb
            | Self::SqlServer
            | Self::ClickHouse
            | Self::BigQuery
            | Self::Databricks => c.is_ascii_alphabetic() || c == '_',
        });
        let rest_ok = chars.all(|c| match self {
            Self::Postgres => c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$',
            Self::Ansi => c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_',
            Self::MySql => c.is_ascii_alphanumeric() || c == '_' || c == '$',
            Self::Sqlite
            | Self::DuckDb
            | Self::SqlServer
            | Self::ClickHouse
            | Self::BigQuery
            | Self::Databricks => c.is_ascii_alphanumeric() || c == '_',
        });
        if first_ok && rest_ok && !is_reserved(name, self) {
            return Cow::Borrowed(name);
        }
        let (open, close) = self.ident_quotes();
        let escaped = match self {
            // A backslash escapes the next character inside BigQuery quotes.
            Self::BigQuery => name.replace('\\', "\\\\").replace('`', "\\`"),
            // …and inside ClickHouse quotes, which also take a doubled quote.
            Self::ClickHouse => name
                .replace(close, &format!("{close}{close}"))
                .replace('\\', "\\\\"),
            _ => name.replace(close, &format!("{close}{close}")),
        };
        Cow::Owned(format!("{open}{escaped}{close}"))
    }

    /// The character that delimits quoted identifiers: the opening one where they differ.
    #[must_use]
    pub const fn ident_quote(self) -> char {
        self.ident_quotes().0
    }

    /// The opening and closing characters of quoted identifiers: `[name]` in Transact-SQL.
    #[must_use]
    pub const fn ident_quotes(self) -> (char, char) {
        match self {
            Self::MySql | Self::ClickHouse | Self::BigQuery | Self::Databricks => ('`', '`'),
            Self::Postgres | Self::Sqlite | Self::DuckDb | Self::Ansi => ('"', '"'),
            Self::SqlServer => ('[', ']'),
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
    /// (`NO_BACKSLASH_ESCAPES` off), and ClickHouse always, so they are doubled there. Transact-SQL literals are
    /// `N'...'`, so that text outside the database's code page survives.
    #[must_use]
    pub fn quote_literal(self, value: &str) -> String {
        let mut out = String::with_capacity(value.len() + 3);
        if self == Self::SqlServer {
            out.push('N');
        }
        out.push('\'');
        // BigQuery and Spark read `''` as two strings, not an escaped quote.
        let backslashes = matches!(self, Self::BigQuery | Self::Databricks);
        for c in value.chars() {
            match c {
                '\'' if backslashes => out.push_str("\\'"),
                '\'' => out.push_str("''"),
                '\\' if backslashes || matches!(self, Self::MySql | Self::ClickHouse) => {
                    out.push_str("\\\\");
                }
                c => out.push(c),
            }
        }
        out.push('\'');
        out
    }

    /// A boolean literal; SQLite has no boolean type and stores 1 and 0, SQL Server's `bit`
    /// takes them too.
    #[must_use]
    pub const fn bool_literal(self, value: bool) -> &'static str {
        match (self, value) {
            (Self::Sqlite | Self::SqlServer, true) => "1",
            (Self::Sqlite | Self::SqlServer, false) => "0",
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
            Self::MySql | Self::Sqlite | Self::Ansi | Self::Databricks => format!("X'{hex}'"),
            Self::BigQuery => format!("FROM_HEX('{hex}')"),
            Self::DuckDb => format!("from_hex('{hex}')"),
            Self::ClickHouse => format!("unhex('{hex}')"),
            Self::SqlServer => format!("0x{hex}"),
        }
    }

    /// Whether `SET column = DEFAULT` works; SQLite needs the default expression instead.
    #[must_use]
    pub const fn has_default_keyword(self) -> bool {
        !matches!(self, Self::Sqlite)
    }

    /// Whether `SAVEPOINT` works inside a transaction; DuckDB and ClickHouse have none.
    #[must_use]
    pub const fn has_savepoints(self) -> bool {
        !matches!(
            self,
            Self::DuckDb | Self::ClickHouse | Self::Ansi | Self::BigQuery | Self::Databricks
        )
    }

    /// The statements that open a transaction in which the database refuses to write, where
    /// it has one (PostgreSQL, MySQL and MariaDB `READ ONLY` transactions, SQLite's
    /// `query_only`), or else a plain transaction: [`read_only_end`](Self::read_only_end)
    /// always rolls it back, so a write that got through is undone. SQL Server waits 5
    /// seconds at most for a lock meanwhile, so such a write cannot hang on other sessions.
    #[must_use]
    pub const fn read_only_begin(self) -> &'static [&'static str] {
        match self {
            Self::Postgres | Self::MySql => &["START TRANSACTION READ ONLY"],
            Self::Sqlite => &["PRAGMA query_only = ON", "BEGIN"],
            Self::DuckDb => &["BEGIN TRANSACTION"],
            Self::SqlServer => &["SET LOCK_TIMEOUT 5000", "BEGIN TRANSACTION"],
            // ClickHouse has no transactions, and a session's `readonly` setting cannot be
            // turned off again; with an ADBC driver transactions vary. Only the statement
            // check stops writes.
            Self::ClickHouse | Self::Ansi | Self::BigQuery | Self::Databricks => &[],
        }
    }

    /// The statements that close what [`read_only_begin`](Self::read_only_begin) opened,
    /// rolling it back.
    #[must_use]
    pub const fn read_only_end(self) -> &'static [&'static str] {
        match self {
            Self::Sqlite => &["ROLLBACK", "PRAGMA query_only = OFF"],
            Self::SqlServer => &["ROLLBACK", "SET LOCK_TIMEOUT -1"],
            Self::Postgres | Self::MySql | Self::DuckDb => &["ROLLBACK"],
            Self::ClickHouse | Self::Ansi | Self::BigQuery | Self::Databricks => &[],
        }
    }

    /// Whether the transaction of [`read_only_begin`](Self::read_only_begin) refuses writes,
    /// rather than only rolling them back afterwards.
    #[must_use]
    pub const fn refuses_writes_when_read_only(self) -> bool {
        matches!(self, Self::Postgres | Self::MySql | Self::Sqlite)
    }

    /// An `INSERT` of a row where every column takes its default.
    #[must_use]
    pub fn insert_defaults(self, table: &str) -> String {
        match self {
            Self::MySql => format!("INSERT INTO {table} () VALUES ()"),
            // An empty JSON object leaves every column to its default.
            Self::ClickHouse => format!("INSERT INTO {table} FORMAT JSONEachRow {{}}"),
            Self::Postgres
            | Self::Sqlite
            | Self::DuckDb
            | Self::SqlServer
            | Self::Ansi
            | Self::BigQuery
            | Self::Databricks => format!("INSERT INTO {table} DEFAULT VALUES"),
        }
    }

    /// A query for about how many rows a table has, read from the statistics where the
    /// database keeps them (SQLite counts them). Its value is NULL when unknown.
    #[must_use]
    pub fn row_estimate_sql(self, schema: Option<&str>, table: &str) -> String {
        match self {
            // A count could scan a warehouse table: unknown.
            Self::Ansi | Self::BigQuery | Self::Databricks => "SELECT NULL".to_owned(),
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
                 WHERE database_name = current_database()                  AND lower(schema_name) = lower({}) AND lower(table_name) = lower({})",
                schema.map_or_else(|| "current_schema()".to_owned(), |s| self.quote_literal(s)),
                self.quote_literal(table)
            ),
            Self::ClickHouse => format!(
                "SELECT total_rows FROM system.tables WHERE database = {} AND name = {}",
                schema.map_or_else(|| "currentDatabase()".to_owned(), |s| self.quote_literal(s)),
                self.quote_literal(table)
            ),
            // Heaps (index 0) or clustered indexes (index 1) hold the rows.
            Self::SqlServer => format!(
                "SELECT SUM(p.rows) FROM sys.partitions p \
                 WHERE p.object_id = OBJECT_ID({}) AND p.index_id IN (0, 1)",
                self.quote_literal(&self.qualified(schema, table))
            ),
        }
    }

    /// The clause that caps a query's rows, appended after `ORDER BY`. Transact-SQL needs
    /// the `ORDER BY`: use [`Dialect::limited`] for queries that may have none.
    #[must_use]
    pub fn limit_clause(self, limit: u64) -> String {
        match self {
            Self::SqlServer => format!("OFFSET 0 ROWS FETCH NEXT {limit} ROWS ONLY"),
            _ => format!("LIMIT {limit}"),
        }
    }

    /// `select` (one `SELECT`, without set operations, a row cap or a trailing `;`)
    /// returning at most `limit` rows: `SELECT [DISTINCT] TOP (n)` in Transact-SQL, a
    /// `LIMIT` clause elsewhere. Transact-SQL queries that do not start with `SELECT` are
    /// wrapped in a derived table.
    #[must_use]
    pub fn limited(self, select: &str, limit: u64) -> String {
        if self != Self::SqlServer {
            return format!("{select} LIMIT {limit}");
        }
        let mut words = Lexer::new(select, self).filter(|t| !t.is_trivia());
        let is = |t: &Token, word: &str| {
            t.kind == TokenKind::Word && t.text(select).eq_ignore_ascii_case(word)
        };
        match words.next() {
            Some(first) if is(&first, "SELECT") => {
                let at = match words.next() {
                    Some(t) if is(&t, "DISTINCT") || is(&t, "ALL") => t.end,
                    _ => first.end,
                };
                format!("{} TOP ({limit}){}", &select[..at], &select[at..])
            }
            _ => format!("SELECT TOP ({limit}) * FROM ({select}) limited"),
        }
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
            // A binary or case-sensitive collation would make LIKE case-sensitive.
            Self::MySql | Self::SqlServer | Self::Ansi | Self::Databricks => {
                format!("LOWER({expr}) LIKE LOWER({pattern}) ESCAPE '!'")
            }
            Self::Sqlite => format!("{expr} LIKE {pattern} ESCAPE '!'"),
            // No `ESCAPE` clause: LIKE escapes with a backslash. The replacement `\\\1`
            // writes one before the escaped character; the literal doubles each backslash.
            Self::ClickHouse => {
                format!(r"{expr} ILIKE replaceRegexpAll({pattern}, '!(.)', '\\\\\\1')")
            }
            // No `ESCAPE` clause either: a raw string writes the backslash.
            Self::BigQuery => {
                format!(r"LOWER({expr}) LIKE LOWER(REGEXP_REPLACE({pattern}, r'!(.)', r'\\\1'))")
            }
        }
    }

    /// A condition true for the rows where any of `columns`, read as text, contains `needle`.
    #[must_use]
    pub fn any_column_contains(self, columns: &[&str], needle: &str) -> String {
        let text = match self {
            Self::Postgres | Self::Sqlite | Self::DuckDb => "TEXT",
            Self::MySql => "CHAR",
            Self::SqlServer => "NVARCHAR(MAX)",
            Self::Ansi => "VARCHAR",
            Self::BigQuery | Self::Databricks => "STRING",
            // `toString` keeps NULLs, where `CAST(NULL AS String)` fails.
            Self::ClickHouse => "",
        };
        let as_text = |c: &str| match self {
            Self::ClickHouse => format!("toString({})", self.quote_ident(c)),
            _ => format!("CAST({} AS {text})", self.quote_ident(c)),
        };
        columns
            .iter()
            .map(|c| self.contains(&as_text(c), needle))
            .collect::<Vec<_>>()
            .join(" OR ")
    }

    /// The column type that holds values of `data_type`, e.g. for a table created to import
    /// a file. Types the dialect lacks (nested values, intervals) fall back to text.
    #[must_use]
    pub fn column_type(self, data_type: &DataType) -> String {
        use DataType as T;
        let text = match self {
            Self::Postgres | Self::Sqlite | Self::MySql => "TEXT",
            Self::DuckDb | Self::Ansi => "VARCHAR",
            Self::SqlServer => "NVARCHAR(MAX)",
            Self::ClickHouse => return Self::clickhouse_type(data_type),
            Self::BigQuery => return Self::bigquery_type(data_type),
            Self::Databricks => return Self::databricks_type(data_type),
        };
        let name = match (data_type, self) {
            (T::Boolean, Self::SqlServer) => "BIT",
            (T::Boolean, _) => "BOOLEAN",
            (t, Self::Sqlite) if t.is_integer() => "INTEGER",
            (T::UInt64, Self::DuckDb) => "UBIGINT",
            (T::UInt64, Self::MySql) => "BIGINT UNSIGNED",
            (T::UInt64, _) => "NUMERIC(20, 0)",
            (T::Int8 | T::Int16 | T::UInt8, _) => "SMALLINT",
            (T::Int32 | T::UInt16, Self::MySql | Self::SqlServer) => "INT",
            (T::Int32 | T::UInt16, _) => "INTEGER",
            (T::Int64 | T::UInt32, _) => "BIGINT",
            (T::Float16 | T::Float32, Self::MySql) | (T::Float64, Self::SqlServer) => "FLOAT",
            (T::Float16 | T::Float32 | T::Float64, Self::Sqlite) | (T::Float16 | T::Float32, _) => {
                "REAL"
            }
            (T::Float64, Self::Postgres) => "DOUBLE PRECISION",
            (T::Float64, _) => "DOUBLE",
            (
                T::Decimal32(p, s) | T::Decimal64(p, s) | T::Decimal128(p, s) | T::Decimal256(p, s),
                _,
            ) => {
                if self == Self::Sqlite {
                    return "NUMERIC".to_owned();
                }
                let max = if self == Self::MySql { 65 } else { 38 };
                return format!("DECIMAL({}, {s})", (*p).min(max));
            }
            (T::Binary | T::LargeBinary | T::BinaryView | T::FixedSizeBinary(_), dialect) => {
                match dialect {
                    Self::Postgres => "BYTEA",
                    Self::MySql => "LONGBLOB",
                    Self::SqlServer => "VARBINARY(MAX)",
                    Self::Ansi => "VARBINARY",
                    // ClickHouse, BigQuery and Databricks returned above.
                    Self::Sqlite
                    | Self::DuckDb
                    | Self::ClickHouse
                    | Self::BigQuery
                    | Self::Databricks => "BLOB",
                }
            }
            (T::Date32 | T::Date64, _) => "DATE",
            (T::Time32(_) | T::Time64(_), dialect) if dialect != Self::Sqlite => "TIME",
            (T::Timestamp(..), Self::MySql) => "DATETIME(6)",
            (T::Timestamp(_, None), Self::SqlServer) => "DATETIME2",
            (T::Timestamp(_, Some(_)), Self::SqlServer) => "DATETIMEOFFSET",
            (T::Timestamp(_, Some(_)), Self::Postgres | Self::DuckDb) => "TIMESTAMPTZ",
            (T::Timestamp(_, Some(_)), Self::Ansi) => "TIMESTAMP WITH TIME ZONE",
            (T::Timestamp(..), _) => "TIMESTAMP",
            (T::Interval(_), Self::Postgres | Self::DuckDb) => "INTERVAL",
            _ => text,
        };
        name.to_owned()
    }

    /// [`column_type`](Self::column_type) in `BigQuery`: one integer and one float type.
    fn bigquery_type(data_type: &DataType) -> String {
        use DataType as T;
        let name = match data_type {
            T::Boolean => "BOOL",
            t if t.is_integer() && *t != T::UInt64 => "INT64",
            T::UInt64 => "NUMERIC",
            T::Float16 | T::Float32 | T::Float64 => "FLOAT64",
            T::Decimal32(p, s) | T::Decimal64(p, s) | T::Decimal128(p, s) | T::Decimal256(p, s) => {
                let kind = if *p > 38 || *s > 9 {
                    "BIGNUMERIC"
                } else {
                    "NUMERIC"
                };
                return format!("{kind}({}, {s})", (*p).min(76));
            }
            T::Binary | T::LargeBinary | T::BinaryView | T::FixedSizeBinary(_) => "BYTES",
            T::Date32 | T::Date64 => "DATE",
            T::Time32(_) | T::Time64(_) => "TIME",
            T::Timestamp(_, Some(_)) => "TIMESTAMP",
            T::Timestamp(_, None) => "DATETIME",
            _ => "STRING",
        };
        name.to_owned()
    }

    /// [`column_type`](Self::column_type) in Spark SQL, which has no time-of-day type.
    fn databricks_type(data_type: &DataType) -> String {
        use DataType as T;
        let name = match data_type {
            T::Boolean => "BOOLEAN",
            T::Int8 | T::Int16 | T::UInt8 => "SMALLINT",
            T::Int32 | T::UInt16 => "INT",
            T::Int64 | T::UInt32 => "BIGINT",
            T::UInt64 => "DECIMAL(20, 0)",
            T::Float16 | T::Float32 => "FLOAT",
            T::Float64 => "DOUBLE",
            T::Decimal32(p, s) | T::Decimal64(p, s) | T::Decimal128(p, s) | T::Decimal256(p, s) => {
                return format!("DECIMAL({}, {s})", (*p).min(38));
            }
            T::Binary | T::LargeBinary | T::BinaryView | T::FixedSizeBinary(_) => "BINARY",
            T::Date32 | T::Date64 => "DATE",
            T::Timestamp(_, Some(_)) => "TIMESTAMP",
            T::Timestamp(_, None) => "TIMESTAMP_NTZ",
            _ => "STRING",
        };
        name.to_owned()
    }

    /// [`column_type`](Self::column_type) in ClickHouse, whose type names are its own.
    fn clickhouse_type(data_type: &DataType) -> String {
        use DataType as T;
        let name = match data_type {
            T::Boolean => "Bool",
            T::Int8 => "Int8",
            T::Int16 => "Int16",
            T::Int32 => "Int32",
            T::Int64 => "Int64",
            T::UInt8 => "UInt8",
            T::UInt16 => "UInt16",
            T::UInt32 => "UInt32",
            T::UInt64 => "UInt64",
            T::Float16 | T::Float32 => "Float32",
            T::Float64 => "Float64",
            T::Decimal32(p, s) | T::Decimal64(p, s) | T::Decimal128(p, s) | T::Decimal256(p, s) => {
                return format!("Decimal({}, {s})", (*p).min(76));
            }
            T::Date32 | T::Date64 => "Date32",
            T::Timestamp(..) => "DateTime64(6)",
            _ => "String",
        };
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_types_of_arrow_types() {
        use platypus_core::arrow_schema::TimeUnit;
        let zoned = DataType::Timestamp(TimeUnit::Microsecond, Some("+00:00".into()));
        let cases = [
            (
                DataType::Int64,
                ["BIGINT", "BIGINT", "INTEGER", "BIGINT", "BIGINT"],
            ),
            (
                DataType::Boolean,
                ["BOOLEAN", "BOOLEAN", "BOOLEAN", "BOOLEAN", "BIT"],
            ),
            (
                DataType::Float64,
                ["DOUBLE PRECISION", "DOUBLE", "REAL", "DOUBLE", "FLOAT"],
            ),
            (
                DataType::Decimal128(70, 2),
                [
                    "DECIMAL(38, 2)",
                    "DECIMAL(65, 2)",
                    "NUMERIC",
                    "DECIMAL(38, 2)",
                    "DECIMAL(38, 2)",
                ],
            ),
            (
                DataType::Utf8,
                ["TEXT", "TEXT", "TEXT", "VARCHAR", "NVARCHAR(MAX)"],
            ),
            (
                zoned,
                [
                    "TIMESTAMPTZ",
                    "DATETIME(6)",
                    "TIMESTAMP",
                    "TIMESTAMPTZ",
                    "DATETIMEOFFSET",
                ],
            ),
            (DataType::Date32, ["DATE"; 5]),
        ];
        let dialects = [
            Dialect::Postgres,
            Dialect::MySql,
            Dialect::Sqlite,
            Dialect::DuckDb,
            Dialect::SqlServer,
        ];
        for (data_type, expected) in cases {
            for (dialect, want) in dialects.iter().zip(expected) {
                assert_eq!(
                    dialect.column_type(&data_type),
                    want,
                    "{data_type} in {dialect:?}"
                );
            }
        }
    }

    #[test]
    fn sorted_reserved_words() {
        for list in [RESERVED, TSQL_RESERVED] {
            assert!(list.windows(2).all(|w| w[0] < w[1]), "{list:?}");
        }
    }

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
            Dialect::SqlServer.contains("c", "[a]"),
            "LOWER(c) LIKE LOWER(N'%![a]%') ESCAPE '!'"
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
    fn limits_rows() {
        assert_eq!(Dialect::DuckDb.limit_clause(5), "LIMIT 5");
        assert_eq!(Dialect::MySql.limit_clause(5), "LIMIT 5");
    }

    #[test]
    fn sql_server_rules() {
        let ms = Dialect::SqlServer;
        assert_eq!(ms.quote_ident("Orders"), "Orders");
        assert_eq!(ms.quote_ident("order"), "[order]");
        assert_eq!(ms.quote_ident("top"), "[top]");
        assert_eq!(ms.quote_ident("a]b c"), "[a]]b c]");
        assert_eq!(Dialect::Postgres.quote_ident("top"), "top");
        assert_eq!(ms.qualified(Some("dbo"), "my table"), "dbo.[my table]");
        assert_eq!(ms.quote_literal("it's \\ ok"), "N'it''s \\ ok'");
        assert_eq!(ms.bool_literal(true), "1");
        assert_eq!(ms.binary_literal("00ff"), "0x00ff");
        assert_eq!(ms.insert_defaults("t"), "INSERT INTO t DEFAULT VALUES");
        assert_eq!(ms.limit_clause(5), "OFFSET 0 ROWS FETCH NEXT 5 ROWS ONLY");
        assert_eq!(
            ms.limited("SELECT a FROM t ORDER BY a", 10),
            "SELECT TOP (10) a FROM t ORDER BY a"
        );
        assert_eq!(
            ms.limited("/* c */ select 1 FROM t", 3),
            "/* c */ select TOP (3) 1 FROM t"
        );
        assert_eq!(
            ms.limited("SELECT DISTINCT a FROM t", 3),
            "SELECT DISTINCT TOP (3) a FROM t"
        );
        assert_eq!(
            ms.limited("(SELECT 1 x)", 2),
            "SELECT TOP (2) * FROM ((SELECT 1 x)) limited"
        );
        assert_eq!(Dialect::MySql.limited("SELECT 1", 2), "SELECT 1 LIMIT 2");
        assert_eq!(
            ms.contains("name", "X"),
            "LOWER(name) LIKE LOWER(N'%X%') ESCAPE '!'"
        );
        assert!(
            ms.any_column_contains(&["id"], "1")
                .contains("CAST(id AS NVARCHAR(MAX))")
        );
        assert!(
            ms.row_estimate_sql(Some("dbo"), "t")
                .contains("OBJECT_ID(N'dbo.t')")
        );
    }

    #[test]
    fn clickhouse_rules() {
        let ch = Dialect::ClickHouse;
        assert_eq!(ch.quote_ident("Orders"), "Orders");
        assert_eq!(ch.quote_ident("select"), "`select`");
        assert_eq!(ch.quote_ident("a`b\\c"), "`a``b\\\\c`");
        assert_eq!(ch.quote_literal("it's \\"), "'it''s \\\\'");
        assert_eq!(ch.bool_literal(false), "FALSE");
        assert_eq!(ch.binary_literal("00ff"), "unhex('00ff')");
        assert!(!ch.has_savepoints() && ch.has_default_keyword());
        assert_eq!(
            ch.insert_defaults("t"),
            "INSERT INTO t FORMAT JSONEachRow {}"
        );
        assert_eq!(ch.limited("SELECT 1", 2), "SELECT 1 LIMIT 2");
        assert_eq!(
            ch.row_estimate_sql(None, "t"),
            "SELECT total_rows FROM system.tables WHERE database = currentDatabase() AND name = 't'"
        );
        assert_eq!(
            ch.contains("c", "a%"),
            r"c ILIKE replaceRegexpAll('%a!%%', '!(.)', '\\\\\\1')"
        );
        assert!(
            ch.any_column_contains(&["id", "n"], "x")
                .starts_with("toString(id) ILIKE")
        );
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
            sql.ends_with(
                "lower(schema_name) = lower('main') AND lower(table_name) = lower('orders')"
            ),
            "{sql}"
        );
        assert!(
            duck.row_estimate_sql(None, "t")
                .contains("lower(schema_name) = lower(current_schema())")
        );
    }

    #[test]
    fn bridge_dialects_of_drivers() {
        assert_eq!(Dialect::from(DriverKind::Snowflake), Dialect::Ansi);
        assert_eq!(Dialect::from(DriverKind::Trino), Dialect::Ansi);
        assert_eq!(Dialect::from(DriverKind::Adbc), Dialect::Ansi);
        assert_eq!(Dialect::from(DriverKind::BigQuery), Dialect::BigQuery);
        assert_eq!(Dialect::from(DriverKind::Databricks), Dialect::Databricks);
    }

    #[test]
    fn bridge_dialects_quote_idents() {
        // Snowflake folds bare names to upper case and Trino to lower case: only names
        // that read the same either way stay bare.
        assert_eq!(Dialect::Ansi.quote_ident("ORDERS"), "ORDERS");
        assert_eq!(Dialect::Ansi.quote_ident("orders"), "\"orders\"");
        assert_eq!(Dialect::Ansi.quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(Dialect::BigQuery.quote_ident("orders"), "orders");
        assert_eq!(Dialect::BigQuery.quote_ident("a b"), "`a b`");
        assert_eq!(Dialect::BigQuery.quote_ident("a`b\\c"), "`a\\`b\\\\c`");
        assert_eq!(Dialect::Databricks.quote_ident("a`b"), "`a``b`");
        assert_eq!(Dialect::BigQuery.quote_ident("select"), "`select`");
    }

    #[test]
    fn bridge_dialects_literals_and_limits() {
        assert_eq!(Dialect::Ansi.quote_literal("it's"), "'it''s'");
        assert_eq!(Dialect::BigQuery.quote_literal("it's \\"), "'it\\'s \\\\'");
        assert_eq!(Dialect::Databricks.quote_literal("it's"), "'it\\'s'");
        assert_eq!(Dialect::Ansi.binary_literal("0aff"), "X'0aff'");
        assert_eq!(Dialect::BigQuery.binary_literal("0aff"), "FROM_HEX('0aff')");
        assert_eq!(Dialect::Databricks.binary_literal("0aff"), "X'0aff'");
        for d in [Dialect::Ansi, Dialect::BigQuery, Dialect::Databricks] {
            assert_eq!(d.limit_clause(5), "LIMIT 5");
            assert_eq!(d.limited("SELECT 1", 5), "SELECT 1 LIMIT 5");
            assert_eq!(d.read_only_begin(), [] as [&str; 0]);
            assert!(!d.has_savepoints());
            // No cheap estimate: unknown.
            assert_eq!(d.row_estimate_sql(Some("s"), "t"), "SELECT NULL");
        }
        assert_eq!(
            Dialect::Ansi.contains("x", "a%"),
            "LOWER(x) LIKE LOWER('%a!%%') ESCAPE '!'"
        );
        assert_eq!(
            Dialect::Databricks.contains("x", "a"),
            "LOWER(x) LIKE LOWER('%a%') ESCAPE '!'"
        );
        // BigQuery's LIKE has no ESCAPE clause: `!x` becomes `\x`.
        assert_eq!(
            Dialect::BigQuery.contains("x", "a"),
            "LOWER(x) LIKE LOWER(REGEXP_REPLACE('%a%', r'!(.)', r'\\\\\\1'))"
        );
    }

    #[test]
    fn bridge_column_types() {
        use platypus_core::arrow_schema::TimeUnit;
        let zoned = DataType::Timestamp(TimeUnit::Microsecond, Some("+00:00".into()));
        let naive = DataType::Timestamp(TimeUnit::Microsecond, None);
        let cases: [(DataType, [&str; 3]); 9] = [
            (DataType::Int32, ["INTEGER", "INT64", "INT"]),
            (DataType::Int64, ["BIGINT", "INT64", "BIGINT"]),
            (DataType::Boolean, ["BOOLEAN", "BOOL", "BOOLEAN"]),
            (DataType::Float64, ["DOUBLE", "FLOAT64", "DOUBLE"]),
            (
                DataType::Decimal128(70, 2),
                ["DECIMAL(38, 2)", "BIGNUMERIC(70, 2)", "DECIMAL(38, 2)"],
            ),
            (DataType::Utf8, ["VARCHAR", "STRING", "STRING"]),
            (DataType::Binary, ["VARBINARY", "BYTES", "BINARY"]),
            (
                zoned,
                ["TIMESTAMP WITH TIME ZONE", "TIMESTAMP", "TIMESTAMP"],
            ),
            (naive, ["TIMESTAMP", "DATETIME", "TIMESTAMP_NTZ"]),
        ];
        for (t, expected) in cases {
            let got =
                [Dialect::Ansi, Dialect::BigQuery, Dialect::Databricks].map(|d| d.column_type(&t));
            assert_eq!(got, expected.map(str::to_owned), "{t}");
        }
    }
}
