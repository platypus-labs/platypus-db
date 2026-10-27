//! What differs between SQL dialects when generating SQL: quoting of identifiers and
//! literals, boolean and binary values, defaults, row limits. Features that build SQL (data editing, DDL, import,
//! the data viewer) go through these methods instead of matching on the driver.
//!
//! `apps/desktop/src/sqlIdent.ts` mirrors [`Dialect::quote_ident`] for the UI; keep the two
//! in sync.

use std::borrow::Cow;
use std::fmt::Write as _;

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
    /// Standard SQL with `"…"` names: Trino and generic ADBC drivers.
    Ansi,
    /// Standard SQL with `"…"` names that folds bare names to upper case; `'…'` strings
    /// take backslash escapes (`\'`, `\\`), and `//` starts a comment.
    Snowflake,
    /// `GoogleSQL`: `` `…` `` names, `"…"` strings, backslash escapes in both.
    BigQuery,
    /// Spark SQL on Databricks: `` `…` `` names (a backtick doubled inside), `"…"` strings
    /// with backslash escapes.
    Databricks,
    /// Oracle Database: bare names fold to upper case, rows capped with `FETCH FIRST`,
    /// no boolean literals before 23ai.
    Oracle,
    /// IBM Db2: bare names fold to upper case, rows capped with `FETCH FIRST`.
    Db2,
}

impl From<DriverKind> for Dialect {
    fn from(kind: DriverKind) -> Self {
        match kind {
            // MongoDB speaks no SQL: the app never builds SQL for it (the editor, data
            // viewer and navigator write mongosh instead), so any dialect would do here.
            DriverKind::Postgres | DriverKind::Mongo => Self::Postgres,
            DriverKind::Snowflake => Self::Snowflake,
            DriverKind::Trino | DriverKind::Adbc | DriverKind::Odbc => Self::Ansi,
            DriverKind::Oracle => Self::Oracle,
            DriverKind::Db2 => Self::Db2,
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
    "CURRENT_CATALOG",
    "CURRENT_DATE",
    "CURRENT_ROLE",
    "CURRENT_SCHEMA",
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
    "LOCALTIME",
    "LOCALTIMESTAMP",
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
    "SESSION_USER",
    "SET",
    "SHOW",
    "SIMILAR",
    "SOME",
    "SUMMARIZE",
    "SYMMETRIC",
    "SYSTEM_USER",
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
    "SETUSER",
    "SHUTDOWN",
    "STATISTICS",
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

/// Further words that Oracle reserves (`V$RESERVED_WORDS` with `RESERVED = 'Y'`).
const ORACLE_RESERVED: &[&str] = &[
    "ACCESS",
    "ADD",
    "ALTER",
    "AUDIT",
    "CHAR",
    "CLUSTER",
    "COMMENT",
    "COMPRESS",
    "CONNECT",
    "CURRENT",
    "DATE",
    "DECIMAL",
    "EXCLUSIVE",
    "FILE",
    "FLOAT",
    "IDENTIFIED",
    "IMMEDIATE",
    "INCREMENT",
    "INITIAL",
    "INTEGER",
    "LEVEL",
    "LOCK",
    "LONG",
    "MAXEXTENTS",
    "MINUS",
    "MLSLABEL",
    "MODE",
    "MODIFY",
    "NOAUDIT",
    "NOCOMPRESS",
    "NOWAIT",
    "NUMBER",
    "OF",
    "OFFLINE",
    "ONLINE",
    "OPTION",
    "PCTFREE",
    "PRIOR",
    "PUBLIC",
    "RAW",
    "RENAME",
    "RESOURCE",
    "REVOKE",
    "ROWID",
    "ROWNUM",
    "SESSION",
    "SHARE",
    "SIZE",
    "SMALLINT",
    "START",
    "SUCCESSFUL",
    "SYNONYM",
    "SYSDATE",
    "TRIGGER",
    "UID",
    "VALIDATE",
    "VARCHAR",
    "VARCHAR2",
    "VIEW",
    "WHENEVER",
];

/// Further words that MySQL and MariaDB reserve (`information_schema.KEYWORDS` with
/// `RESERVED = 1` on MySQL 8.4; the keywords MariaDB 11.4 rejects as bare table names).
const MYSQL_RESERVED: &[&str] = &[
    "ACCESSIBLE",
    "ADD",
    "ALTER",
    "ASENSITIVE",
    "BEFORE",
    "BIGINT",
    "BLOB",
    "CALL",
    "CASCADE",
    "CHANGE",
    "CHAR",
    "CHARACTER",
    "CONDITION",
    "CONTINUE",
    "CONVERT",
    "CUBE",
    "CUME_DIST",
    "CURSOR",
    "DATABASE",
    "DATABASES",
    "DAY_HOUR",
    "DAY_MICROSECOND",
    "DAY_MINUTE",
    "DAY_SECOND",
    "DEC",
    "DECIMAL",
    "DECLARE",
    "DELAYED",
    "DELETE_DOMAIN_ID",
    "DENSE_RANK",
    "DETERMINISTIC",
    "DISTINCTROW",
    "DIV",
    "DOUBLE",
    "DO_DOMAIN_IDS",
    "DUAL",
    "EACH",
    "ELSEIF",
    "EMPTY",
    "ENCLOSED",
    "ESCAPED",
    "EXIT",
    "EXPLAIN",
    "FIRST_VALUE",
    "FLOAT",
    "FLOAT4",
    "FLOAT8",
    "FORCE",
    "FULLTEXT",
    "FUNCTION",
    "GENERATED",
    "GET",
    "GROUPING",
    "GROUPS",
    "HIGH_PRIORITY",
    "HOUR_MICROSECOND",
    "HOUR_MINUTE",
    "HOUR_SECOND",
    "IF",
    "IGNORE",
    "IGNORE_DOMAIN_IDS",
    "INFILE",
    "INOUT",
    "INSENSITIVE",
    "INT",
    "INT1",
    "INT2",
    "INT3",
    "INT4",
    "INT8",
    "INTEGER",
    "INTERVAL",
    "IO_AFTER_GTIDS",
    "IO_BEFORE_GTIDS",
    "ITERATE",
    "JSON_TABLE",
    "KEYS",
    "KILL",
    "LAG",
    "LAST_VALUE",
    "LEAD",
    "LEAVE",
    "LINEAR",
    "LINES",
    "LOAD",
    "LOCK",
    "LONG",
    "LONGBLOB",
    "LONGTEXT",
    "LOOP",
    "LOW_PRIORITY",
    "MASTER_DEMOTE_TO_REPLICA",
    "MASTER_DEMOTE_TO_SLAVE",
    "MASTER_SSL_VERIFY_SERVER_CERT",
    "MATCH",
    "MAXVALUE",
    "MEDIUMBLOB",
    "MEDIUMINT",
    "MEDIUMTEXT",
    "MIDDLEINT",
    "MINUTE_MICROSECOND",
    "MINUTE_SECOND",
    "MOD",
    "MODIFIES",
    "NO_WRITE_TO_BINLOG",
    "NTH_VALUE",
    "NTILE",
    "NUMERIC",
    "OF",
    "OPTIMIZE",
    "OPTIMIZER_COSTS",
    "OPTION",
    "OPTIONALLY",
    "OUT",
    "OUTFILE",
    "OVER",
    "PAGE_CHECKSUM",
    "PARSE_VCOL_EXPR",
    "PARTITION",
    "PERCENT_RANK",
    "PORTION",
    "PRECISION",
    "PROCEDURE",
    "PURGE",
    "RANGE",
    "RANK",
    "READ",
    "READS",
    "READ_WRITE",
    "REAL",
    "RECURSIVE",
    "REF_SYSTEM_ID",
    "REGEXP",
    "RELEASE",
    "RENAME",
    "REPEAT",
    "REPLACE",
    "REQUIRE",
    "RESIGNAL",
    "RESTRICT",
    "RETURN",
    "REVOKE",
    "RLIKE",
    "ROW_NUMBER",
    "SCHEMA",
    "SCHEMAS",
    "SECOND_MICROSECOND",
    "SENSITIVE",
    "SEPARATOR",
    "SIGNAL",
    "SMALLINT",
    "SPATIAL",
    "SPECIFIC",
    "SQL",
    "SQLEXCEPTION",
    "SQLSTATE",
    "SQLWARNING",
    "SQL_AFTER_GTIDS",
    "SQL_BEFORE_GTIDS",
    "SQL_BIG_RESULT",
    "SQL_CALC_FOUND_ROWS",
    "SQL_SMALL_RESULT",
    "SSL",
    "STARTING",
    "STATS_AUTO_RECALC",
    "STATS_PERSISTENT",
    "STATS_SAMPLE_PAGES",
    "STORED",
    "STRAIGHT_JOIN",
    "SYSTEM",
    "TERMINATED",
    "TINYBLOB",
    "TINYINT",
    "TINYTEXT",
    "TRIGGER",
    "UNDO",
    "UNLOCK",
    "UNSIGNED",
    "USAGE",
    "USE",
    "UTC_DATE",
    "UTC_TIME",
    "UTC_TIMESTAMP",
    // Not reserved, but MariaDB reads `INSERT INTO value VALUES …` as `VALUE (…)`.
    "VALUE",
    "VARBINARY",
    "VARCHAR",
    "VARCHARACTER",
    "VARYING",
    "VIRTUAL",
    "WHILE",
    "WRITE",
    "XOR",
    "YEAR_MONTH",
    "ZEROFILL",
];

/// Further SQLite keywords that cannot be bare table or column names (every keyword of
/// SQLite 3.4x tried as one).
const SQLITE_RESERVED: &[&str] = &[
    "ADD",
    "ALTER",
    "AUTOINCREMENT",
    "COMMIT",
    "ESCAPE",
    "IF",
    "NOTHING",
    "RAISE",
    "TRANSACTION",
];

pub(crate) fn is_reserved(name: &str, dialect: Dialect) -> bool {
    // The lists are sorted; the `sorted_reserved_words` test keeps them so.
    let upper = name.to_ascii_uppercase();
    let found = |list: &[&str]| list.binary_search(&upper.as_str()).is_ok();
    found(RESERVED)
        || match dialect {
            Dialect::SqlServer => found(TSQL_RESERVED),
            Dialect::Oracle => found(ORACLE_RESERVED),
            Dialect::MySql => found(MYSQL_RESERVED),
            Dialect::Sqlite => found(SQLITE_RESERVED),
            _ => false,
        }
}

/// MySQL text as its UTF-8 bytes in hex, `_utf8mb4 X'…'`: a string of the connection's
/// collation whatever `NO_BACKSLASH_ESCAPES` says.
fn mysql_hex_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 2 + 12);
    out.push_str("_utf8mb4 X'");
    for b in value.bytes() {
        let _ = write!(out, "{b:02x}");
    }
    out.push('\'');
    out
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
            Self::Ansi | Self::Snowflake => c.is_ascii_uppercase() || c == '_',
            // Oracle and Db2 fold bare names to upper case; a name starts with a letter.
            Self::Oracle | Self::Db2 => c.is_ascii_uppercase(),
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
            Self::Ansi | Self::Snowflake | Self::Db2 => {
                c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'
            }
            Self::Oracle => {
                c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '_' | '$' | '#')
            }
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
            Self::Postgres
            | Self::Sqlite
            | Self::DuckDb
            | Self::Ansi
            | Self::Snowflake
            | Self::Oracle
            | Self::Db2 => ('"', '"'),
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

    /// A string literal, read the same whatever the session's settings. ClickHouse always
    /// takes backslashes as escapes, so they are doubled there. Transact-SQL literals are
    /// `N'...'`, so that text outside the database's code page survives.
    ///
    /// Text with a backslash depends on a setting elsewhere, so it is written in a form that
    /// does not: on PostgreSQL an `E'...'` string with the backslashes doubled (a plain
    /// literal takes them as escapes when `standard_conforming_strings` is off, and `\'`
    /// would end it), on MySQL the text's UTF-8 bytes in hex (`NO_BACKSLASH_ESCAPES` decides
    /// whether a backslash escapes).
    #[must_use]
    pub fn quote_literal(self, value: &str) -> String {
        let has_backslash = value.contains('\\');
        if self == Self::MySql && has_backslash {
            return mysql_hex_text(value);
        }
        let mut out = String::with_capacity(value.len() + 3);
        let escape_string = self == Self::Postgres && has_backslash;
        if self == Self::SqlServer {
            out.push('N');
        } else if escape_string {
            out.push('E');
        }
        out.push('\'');
        // BigQuery and Spark read `''` as two strings, not an escaped quote.
        let backslashes = matches!(self, Self::BigQuery | Self::Databricks);
        for c in value.chars() {
            match c {
                '\'' if backslashes => out.push_str("\\'"),
                '\'' => out.push_str("''"),
                // Snowflake reads `\'` as a quote too, so `''` stays correct there.
                '\\' if backslashes
                    || escape_string
                    || matches!(self, Self::ClickHouse | Self::Snowflake) =>
                {
                    out.push_str("\\\\");
                }
                c => out.push(c),
            }
        }
        out.push('\'');
        out
    }

    /// A plain string literal, for where the grammar takes no expression (MySQL's
    /// `COMMENT '…'` and `ENUM('…')`): [`Dialect::quote_literal`], except MySQL text with a
    /// backslash, which is written as when backslashes escape (`NO_BACKSLASH_ESCAPES` off,
    /// the default).
    #[must_use]
    pub fn quote_string(self, value: &str) -> String {
        if self == Self::MySql && value.contains('\\') {
            return format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"));
        }
        self.quote_literal(value)
    }

    /// A boolean literal; SQLite has no boolean type and stores 1 and 0, SQL Server's `bit`
    /// takes them too, and so do Oracle's `NUMBER(1)` flags and its 23ai `BOOLEAN`.
    #[must_use]
    pub const fn bool_literal(self, value: bool) -> &'static str {
        match (self, value) {
            (Self::Sqlite | Self::SqlServer | Self::Oracle, true) => "1",
            (Self::Sqlite | Self::SqlServer | Self::Oracle, false) => "0",
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
            Self::MySql | Self::Sqlite | Self::Ansi | Self::Snowflake | Self::Databricks => {
                format!("X'{hex}'")
            }
            Self::BigQuery => format!("FROM_HEX('{hex}')"),
            Self::DuckDb => format!("from_hex('{hex}')"),
            Self::ClickHouse => format!("unhex('{hex}')"),
            Self::SqlServer => format!("0x{hex}"),
            Self::Oracle => format!("HEXTORAW('{hex}')"),
            Self::Db2 => format!("BX'{hex}'"),
        }
    }

    /// Whether `SET column = DEFAULT` works; SQLite needs the default expression instead.
    #[must_use]
    pub const fn has_default_keyword(self) -> bool {
        !matches!(self, Self::Sqlite)
    }

    /// Whether `SAVEPOINT` and `ROLLBACK TO SAVEPOINT` work inside a transaction; DuckDB and
    /// ClickHouse have none, Db2 another syntax.
    #[must_use]
    pub const fn has_savepoints(self) -> bool {
        !matches!(
            self,
            Self::DuckDb
                | Self::ClickHouse
                | Self::Ansi
                | Self::Snowflake
                | Self::BigQuery
                | Self::Databricks
                | Self::Db2
        )
    }

    /// Whether a savepoint is ended with `RELEASE SAVEPOINT`: Oracle's last until the
    /// transaction ends.
    #[must_use]
    pub const fn releases_savepoints(self) -> bool {
        self.has_savepoints() && !matches!(self, Self::Oracle)
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
            // turned off again; with an ADBC driver transactions vary. Oracle's `SET
            // TRANSACTION READ ONLY` ends with the statement in autocommit mode. Only the
            // statement check stops writes.
            Self::ClickHouse
            | Self::Ansi
            | Self::Snowflake
            | Self::BigQuery
            | Self::Databricks
            | Self::Oracle
            | Self::Db2 => &[],
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
            Self::ClickHouse
            | Self::Ansi
            | Self::Snowflake
            | Self::BigQuery
            | Self::Databricks
            | Self::Oracle
            | Self::Db2 => &[],
        }
    }

    /// Whether the transaction of [`read_only_begin`](Self::read_only_begin) refuses writes,
    /// rather than only rolling them back afterwards.
    #[must_use]
    pub const fn refuses_writes_when_read_only(self) -> bool {
        matches!(self, Self::Postgres | Self::MySql | Self::Sqlite)
    }

    /// An `INSERT` of a row where every column takes its default; `column` is any of the
    /// table's columns, quoted, for Oracle and Db2, which have no `DEFAULT VALUES`.
    #[must_use]
    pub fn insert_defaults(self, table: &str, column: &str) -> String {
        match self {
            Self::MySql => format!("INSERT INTO {table} () VALUES ()"),
            Self::Oracle | Self::Db2 => format!("INSERT INTO {table} ({column}) VALUES (DEFAULT)"),
            // An empty JSON object leaves every column to its default.
            Self::ClickHouse => format!("INSERT INTO {table} FORMAT JSONEachRow {{}}"),
            Self::Postgres
            | Self::Sqlite
            | Self::DuckDb
            | Self::SqlServer
            | Self::Ansi
            | Self::Snowflake
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
            Self::Ansi | Self::Snowflake | Self::BigQuery | Self::Databricks => {
                "SELECT NULL".to_owned()
            }
            // A table never analyzed has no statistics (`reltuples` is -1, or 0 on 0 pages
            // before PostgreSQL 14): its rows are counted, up to a million.
            Self::Postgres => {
                let name = self.qualified(schema, table);
                format!(
                    "SELECT CASE WHEN c.reltuples > 0 OR (c.reltuples = 0 AND c.relpages > 0) \
                     THEN c.reltuples::bigint \
                     ELSE (SELECT count(*) FROM (SELECT 1 FROM {name} LIMIT 1000000) s) END \
                     FROM pg_class c WHERE c.oid = to_regclass({})",
                    self.quote_literal(&name)
                )
            }
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
            Self::Oracle => format!(
                "SELECT NUM_ROWS FROM ALL_TABLES WHERE OWNER = {} AND TABLE_NAME = {}",
                schema.map_or_else(
                    || "SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA')".to_owned(),
                    |s| self.quote_literal(s)
                ),
                self.quote_literal(table)
            ),
            // `CARD` is -1 until statistics are collected.
            Self::Db2 => format!(
                "SELECT CASE WHEN CARD < 0 THEN NULL ELSE CARD END FROM SYSCAT.TABLES \
                 WHERE TABSCHEMA = {} AND TABNAME = {}",
                schema.map_or_else(|| "CURRENT SCHEMA".to_owned(), |s| self.quote_literal(s)),
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
            Self::Oracle | Self::Db2 => format!("FETCH FIRST {limit} ROWS ONLY"),
            _ => format!("LIMIT {limit}"),
        }
    }

    /// `select` (one `SELECT`, without set operations, a row cap or a trailing `;`)
    /// returning at most `limit` rows: `SELECT [DISTINCT] TOP (n)` in Transact-SQL, the
    /// [`limit_clause`](Self::limit_clause) elsewhere. Transact-SQL queries that do not
    /// start with `SELECT` are wrapped in a derived table.
    #[must_use]
    pub fn limited(self, select: &str, limit: u64) -> String {
        if self != Self::SqlServer {
            return format!("{select} {}", self.limit_clause(limit));
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
            Self::MySql
            | Self::SqlServer
            | Self::Ansi
            | Self::Snowflake
            | Self::Databricks
            | Self::Oracle
            | Self::Db2 => format!("LOWER({expr}) LIKE LOWER({pattern}) ESCAPE '!'"),
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
            Self::Ansi | Self::Snowflake => "VARCHAR",
            Self::BigQuery | Self::Databricks => "STRING",
            Self::Oracle => "VARCHAR2(4000)",
            Self::Db2 => "VARCHAR(4000)",
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
            Self::DuckDb | Self::Ansi | Self::Snowflake => "VARCHAR",
            Self::SqlServer => "NVARCHAR(MAX)",
            Self::ClickHouse => return Self::clickhouse_type(data_type),
            Self::BigQuery => return Self::bigquery_type(data_type),
            Self::Databricks => return Self::databricks_type(data_type),
            Self::Oracle => return Self::oracle_type(data_type),
            Self::Db2 => "CLOB",
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
                let max = match self {
                    Self::MySql => 65,
                    Self::Db2 => 31,
                    _ => 38,
                };
                return format!("DECIMAL({}, {s})", (*p).min(max));
            }
            (T::Binary | T::LargeBinary | T::BinaryView | T::FixedSizeBinary(_), dialect) => {
                match dialect {
                    Self::Postgres => "BYTEA",
                    Self::MySql => "LONGBLOB",
                    Self::SqlServer => "VARBINARY(MAX)",
                    Self::Ansi | Self::Snowflake => "VARBINARY",
                    // ClickHouse, BigQuery, Databricks and Oracle returned above.
                    Self::Sqlite
                    | Self::DuckDb
                    | Self::ClickHouse
                    | Self::BigQuery
                    | Self::Databricks
                    | Self::Oracle
                    | Self::Db2 => "BLOB",
                }
            }
            (T::Date32 | T::Date64, _) => "DATE",
            (T::Time32(_) | T::Time64(_), dialect) if dialect != Self::Sqlite => "TIME",
            (T::Timestamp(..), Self::MySql) => "DATETIME(6)",
            (T::Timestamp(_, None), Self::SqlServer) => "DATETIME2",
            (T::Timestamp(_, Some(_)), Self::SqlServer) => "DATETIMEOFFSET",
            (T::Timestamp(_, Some(_)), Self::Postgres | Self::DuckDb) => "TIMESTAMPTZ",
            (T::Timestamp(_, Some(_)), Self::Ansi | Self::Snowflake) => "TIMESTAMP WITH TIME ZONE",
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
                return Self::bigquery_decimal(*p, *s);
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

    /// [`column_type`](Self::column_type) in Oracle: numbers are `NUMBER`, no time-of-day
    /// type, text that may be long is a `CLOB`.
    fn oracle_type(data_type: &DataType) -> String {
        use DataType as T;
        let name = match data_type {
            T::Boolean => "NUMBER(1)",
            T::Int8 | T::Int16 | T::UInt8 => "NUMBER(5)",
            T::Int32 | T::UInt16 => "NUMBER(10)",
            T::Int64 | T::UInt32 => "NUMBER(19)",
            T::UInt64 => "NUMBER(20)",
            T::Float16 | T::Float32 => "BINARY_FLOAT",
            T::Float64 => "BINARY_DOUBLE",
            T::Decimal32(p, s) | T::Decimal64(p, s) | T::Decimal128(p, s) | T::Decimal256(p, s) => {
                return format!("NUMBER({}, {s})", (*p).min(38));
            }
            T::Binary | T::LargeBinary | T::BinaryView | T::FixedSizeBinary(_) => "BLOB",
            T::Date32 | T::Date64 => "DATE",
            T::Timestamp(_, Some(_)) => "TIMESTAMP WITH TIME ZONE",
            T::Timestamp(_, None) => "TIMESTAMP",
            T::Interval(_) => "INTERVAL DAY TO SECOND",
            _ => "CLOB",
        };
        name.to_owned()
    }

    /// A `BigQuery` decimal type for an Arrow precision and scale. `NUMERIC(p, s)` takes
    /// `max(1, s) <= p <= s + 29` and `s <= 9`; `BIGNUMERIC(p, s)` takes `p <= s + 38` and
    /// `s <= 38`, so the precision is cut to fit when the integer digits would not.
    fn bigquery_decimal(precision: u8, scale: i8) -> String {
        // A negative Arrow scale rounds to tens: whole numbers here.
        let scale = u8::try_from(scale).unwrap_or(0);
        let precision = precision.max(scale).max(1);
        if scale <= 9 && precision <= scale + 29 {
            return format!("NUMERIC({precision}, {scale})");
        }
        let scale = scale.min(38);
        let precision = precision.clamp(scale.max(1), scale + 38);
        format!("BIGNUMERIC({precision}, {scale})")
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
                // `DECIMAL(p, s)` takes `0 <= s <= p <= 38`.
                let scale = u8::try_from(*s).unwrap_or(0).min(38);
                let precision = (*p).max(scale).clamp(1, 38);
                return format!("DECIMAL({precision}, {scale})");
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
        for list in [
            RESERVED,
            TSQL_RESERVED,
            ORACLE_RESERVED,
            MYSQL_RESERVED,
            SQLITE_RESERVED,
        ] {
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
    fn quotes_each_dialects_reserved_words() {
        // PostgreSQL reads these as value functions in FROM: a table must be quoted.
        for name in [
            "current_role",
            "current_schema",
            "current_catalog",
            "session_user",
            "system_user",
            "localtime",
            "localtimestamp",
        ] {
            assert_eq!(Dialect::Postgres.quote_ident(name), format!("\"{name}\""));
        }
        for name in ["rank", "range", "interval", "groups", "lead", "read", "div"] {
            assert_eq!(Dialect::MySql.quote_ident(name), format!("`{name}`"));
            assert_eq!(Dialect::Postgres.quote_ident(name), name, "{name}");
        }
        for name in [
            "transaction",
            "commit",
            "add",
            "alter",
            "escape",
            "nothing",
            "autoincrement",
        ] {
            assert_eq!(Dialect::Sqlite.quote_ident(name), format!("\"{name}\""));
        }
        assert_eq!(Dialect::Sqlite.quote_ident("rank"), "rank");
        assert_eq!(Dialect::MySql.quote_ident("transaction"), "transaction");
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
        // Read the same whatever `standard_conforming_strings` says.
        assert_eq!(Dialect::Postgres.quote_literal("a\\b"), "E'a\\\\b'");
        assert_eq!(
            Dialect::Postgres.quote_literal("x\\'; DROP TABLE t; --"),
            "E'x\\\\''; DROP TABLE t; --'"
        );
        assert_eq!(Dialect::DuckDb.quote_literal("a\\b"), "'a\\b'");
        // Bytes, which `NO_BACKSLASH_ESCAPES` does not change; a plain string where the
        // grammar takes nothing else, as backslashes escape by default.
        assert_eq!(
            Dialect::MySql.quote_literal("a\\'b"),
            "_utf8mb4 X'615c2762'"
        );
        assert_eq!(Dialect::MySql.quote_literal("é's"), "'é''s'");
        assert_eq!(Dialect::MySql.quote_string("a\\'b"), "'a\\\\''b'");
        assert_eq!(Dialect::Postgres.quote_string("a\\b"), "E'a\\\\b'");
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
        assert_eq!(ms.insert_defaults("t", "a"), "INSERT INTO t DEFAULT VALUES");
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
            ch.insert_defaults("t", "a"),
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
            Dialect::MySql.insert_defaults("t", "a"),
            "INSERT INTO t () VALUES ()"
        );
        assert_eq!(
            Dialect::DuckDb.insert_defaults("t", "a"),
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
        assert_eq!(Dialect::from(DriverKind::Snowflake), Dialect::Snowflake);
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
        assert_eq!(Dialect::Snowflake.quote_ident("ORDERS"), "ORDERS");
        assert_eq!(Dialect::Snowflake.quote_ident("orders"), "\"orders\"");
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
        assert_eq!(Dialect::Ansi.quote_literal(r"C:\new"), r"'C:\new'");
        // Snowflake reads `\n` in a string as a newline: the backslash is doubled.
        let literal = Dialect::Snowflake.quote_literal(r"C:\new it's");
        assert_eq!(literal, r"'C:\\new it''s'");
        let tokens: Vec<_> = Lexer::new(&literal, Dialect::Snowflake).collect();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::String);
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
    fn oracle_and_db2_rules() {
        let (ora, db2) = (Dialect::Oracle, Dialect::Db2);
        assert_eq!(Dialect::from(DriverKind::Oracle), ora);
        assert_eq!(Dialect::from(DriverKind::Db2), db2);
        assert_eq!(Dialect::from(DriverKind::Odbc), Dialect::Ansi);
        assert!(ora.has_savepoints() && !db2.has_savepoints());
        for d in [ora, db2] {
            assert_eq!(d.quote_ident("ORDERS"), "ORDERS");
            assert_eq!(d.quote_ident("Orders"), "\"Orders\"");
            assert_eq!(d.quote_ident("_X"), "\"_X\"");
            assert_eq!(d.quote_ident("SELECT"), "\"SELECT\"");
            assert_eq!(d.quote_literal("it's \\"), "'it''s \\'");
            assert_eq!(d.limit_clause(5), "FETCH FIRST 5 ROWS ONLY");
            assert_eq!(d.limited("SELECT 1", 5), "SELECT 1 FETCH FIRST 5 ROWS ONLY");
            assert!(!d.releases_savepoints() && d.has_default_keyword());
            assert_eq!(d.read_only_begin(), [] as [&str; 0]);
            assert_eq!(
                d.insert_defaults("T", "ID"),
                "INSERT INTO T (ID) VALUES (DEFAULT)"
            );
            assert_eq!(
                d.contains("X", "a"),
                "LOWER(X) LIKE LOWER('%a%') ESCAPE '!'"
            );
        }
        // Oracle reserves more words, and allows `$` and `#` in bare names.
        assert_eq!(ora.quote_ident("NUMBER"), "\"NUMBER\"");
        assert_eq!(db2.quote_ident("NUMBER"), "NUMBER");
        assert_eq!(ora.quote_ident("A$B#"), "A$B#");
        assert_eq!(db2.quote_ident("A$B"), "\"A$B\"");
        assert_eq!(ora.bool_literal(true), "1");
        assert_eq!(db2.bool_literal(true), "TRUE");
        assert_eq!(ora.binary_literal("0aff"), "HEXTORAW('0aff')");
        assert_eq!(db2.binary_literal("0aff"), "BX'0aff'");
        assert!(
            ora.row_estimate_sql(None, "T")
                .contains("OWNER = SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') AND TABLE_NAME = 'T'")
        );
        assert!(
            db2.row_estimate_sql(Some("S"), "T")
                .ends_with("WHERE TABSCHEMA = 'S' AND TABNAME = 'T'")
        );
        assert!(
            ora.any_column_contains(&["A"], "x")
                .starts_with("LOWER(CAST(A AS VARCHAR2(4000)))")
        );
    }

    #[test]
    fn oracle_and_db2_column_types() {
        use platypus_core::arrow_schema::TimeUnit;
        let zoned = DataType::Timestamp(TimeUnit::Microsecond, Some("+00:00".into()));
        let cases: [(DataType, [&str; 2]); 9] = [
            (DataType::Boolean, ["NUMBER(1)", "BOOLEAN"]),
            (DataType::Int32, ["NUMBER(10)", "INTEGER"]),
            (DataType::Int64, ["NUMBER(19)", "BIGINT"]),
            (DataType::Float32, ["BINARY_FLOAT", "REAL"]),
            (DataType::Float64, ["BINARY_DOUBLE", "DOUBLE"]),
            (
                DataType::Decimal128(40, 2),
                ["NUMBER(38, 2)", "DECIMAL(31, 2)"],
            ),
            (DataType::Utf8, ["CLOB", "CLOB"]),
            (DataType::Binary, ["BLOB", "BLOB"]),
            (zoned, ["TIMESTAMP WITH TIME ZONE", "TIMESTAMP"]),
        ];
        for (t, expected) in cases {
            let got = [Dialect::Oracle, Dialect::Db2].map(|d| d.column_type(&t));
            assert_eq!(got, expected.map(str::to_owned), "{t}");
        }
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
                ["DECIMAL(38, 2)", "BIGNUMERIC(40, 2)", "DECIMAL(38, 2)"],
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

    #[test]
    fn bridge_decimal_bounds() {
        let types = |p, s| {
            let t = DataType::Decimal256(p, s);
            [Dialect::BigQuery, Dialect::Databricks].map(|d| d.column_type(&t))
        };
        let cases = [
            // NUMERIC holds 29 integer digits and 9 decimals.
            ((38, 9), ["NUMERIC(38, 9)", "DECIMAL(38, 9)"]),
            ((10, 2), ["NUMERIC(10, 2)", "DECIMAL(10, 2)"]),
            ((31, 0), ["BIGNUMERIC(31, 0)", "DECIMAL(31, 0)"]),
            ((12, 10), ["BIGNUMERIC(12, 10)", "DECIMAL(12, 10)"]),
            ((76, 38), ["BIGNUMERIC(76, 38)", "DECIMAL(38, 38)"]),
            ((76, 50), ["BIGNUMERIC(76, 38)", "DECIMAL(38, 38)"]),
            ((50, 0), ["BIGNUMERIC(38, 0)", "DECIMAL(38, 0)"]),
            ((5, -2), ["NUMERIC(5, 0)", "DECIMAL(5, 0)"]),
            ((0, 0), ["NUMERIC(1, 0)", "DECIMAL(1, 0)"]),
        ];
        for ((p, s), expected) in cases {
            assert_eq!(types(p, s), expected.map(str::to_owned), "({p}, {s})");
        }
    }
}
