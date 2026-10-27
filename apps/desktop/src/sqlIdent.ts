// Identifier quoting per dialect, without imports so that it can be unit tested
// (tests/sqlIdent.test.mjs). Re-exported by sqlNames.ts.
import type { DriverKind } from "./api";

/** Words that cannot be bare identifiers in at least one supported dialect (`RESERVED` in
 *  `dialect.rs`, like the lists below: `tests/sqlIdent.test.mjs` keeps them the same). */
export const RESERVED = new Set(
  (
    "ALL ANALYSE ANALYZE AND ANTI ANY ARRAY AS ASC ASOF ASYMMETRIC AT AUTHORIZATION BETWEEN " +
    "BINARY BOTH BY CASE CAST CHECK COLLATE COLLATION COLUMN COLUMNS CONCURRENTLY CONSTRAINT " +
    "CREATE CROSS CURRENT_CATALOG CURRENT_DATE CURRENT_ROLE CURRENT_SCHEMA CURRENT_TIME " +
    "CURRENT_TIMESTAMP CURRENT_USER DEFAULT DEFERRABLE DELETE DESC DESCRIBE DISTINCT DO DROP ELSE " +
    "END EXCEPT EXISTS FALSE FETCH FOR FOREIGN FREEZE FROM FULL GLOB GRANT GROUP HAVING ILIKE IN " +
    "INDEX INITIALLY INNER INSERT INTERSECT INTO IS ISNULL JOIN KEY LAMBDA LATERAL LEADING LEFT " +
    "LIKE LIMIT LOCALTIME LOCALTIMESTAMP NATURAL NOT NOTNULL NULL OFFSET ON ONLY OR ORDER OUTER " +
    "OVERLAPS PIVOT PIVOT_LONGER PIVOT_WIDER PLACING POSITIONAL PRIMARY QUALIFY REFERENCES " +
    "RETURNING RIGHT ROW ROWS SELECT SEMI SESSION_USER SET SHOW SIMILAR SOME SUMMARIZE SYMMETRIC " +
    "SYSTEM_USER TABLE TABLESAMPLE THEN TO TRAILING TRUE UNION UNIQUE UNPACK UNPIVOT UPDATE USER " +
    "USING VALUES VARIADIC VERBOSE WHEN WHERE WINDOW WITH"
  ).split(" "),
);

/** Further words that Transact-SQL reserves. */
export const TSQL_RESERVED = new Set(
  (
    "ADD BACKUP BEGIN BREAK BROWSE BULK CASCADE CHECKPOINT CLOSE CLUSTERED COALESCE COMMIT " +
    "COMPUTE CONTAINS CONTAINSTABLE CONTINUE CONVERT CURSOR DATABASE DBCC DEALLOCATE DECLARE DENY " +
    "DISK DISTRIBUTED DOUBLE DUMP ERRLVL ESCAPE EXEC EXECUTE EXIT EXTERNAL FILE FILLFACTOR " +
    "FREETEXT FREETEXTTABLE FUNCTION GOTO HOLDLOCK IDENTITY IDENTITYCOL IDENTITY_INSERT IF KILL " +
    "LINENO LOAD MERGE NATIONAL NOCHECK NONCLUSTERED NULLIF OF OFF OFFSETS OPEN OPENDATASOURCE " +
    "OPENQUERY OPENROWSET OPENXML OPTION OVER PERCENT PIVOT PLAN PRECISION PRINT PROC PROCEDURE " +
    "PUBLIC RAISERROR READ READTEXT RECONFIGURE REPLICATION RESTORE RESTRICT RETURN REVERT REVOKE " +
    "ROLLBACK ROWCOUNT ROWGUIDCOL RULE SAVE SCHEMA SECURITYAUDIT SEMANTICKEYPHRASETABLE SETUSER " +
    "SHUTDOWN STATISTICS TABLESAMPLE TEXTSIZE TOP TRAN TRANSACTION TRIGGER TRUNCATE TRY_CONVERT " +
    "TSEQUAL UNPIVOT UPDATETEXT USE VARYING VIEW WAITFOR WHILE WITHIN WRITETEXT"
  ).split(" "),
);

/** Further words that Oracle reserves. */
export const ORACLE_RESERVED = new Set(
  (
    "ACCESS ADD ALTER AUDIT CHAR CLUSTER COMMENT COMPRESS CONNECT CURRENT DATE DECIMAL EXCLUSIVE " +
    "FILE FLOAT IDENTIFIED IMMEDIATE INCREMENT INITIAL INTEGER LEVEL LOCK LONG MAXEXTENTS MINUS " +
    "MLSLABEL MODE MODIFY NOAUDIT NOCOMPRESS NOWAIT NUMBER OF OFFLINE ONLINE OPTION PCTFREE PRIOR " +
    "PUBLIC RAW RENAME RESOURCE REVOKE ROWID ROWNUM SESSION SHARE SIZE SMALLINT START SUCCESSFUL " +
    "SYNONYM SYSDATE TRIGGER UID VALIDATE VARCHAR VARCHAR2 VIEW WHENEVER"
  ).split(" "),
);

/** Further words that MySQL and MariaDB reserve. */
export const MYSQL_RESERVED = new Set(
  (
    "ACCESSIBLE ADD ALTER ASENSITIVE BEFORE BIGINT BLOB CALL CASCADE CHANGE CHAR CHARACTER " +
    "CONDITION CONTINUE CONVERT CUBE CUME_DIST CURSOR DATABASE DATABASES DAY_HOUR DAY_MICROSECOND " +
    "DAY_MINUTE DAY_SECOND DEC DECIMAL DECLARE DELAYED DELETE_DOMAIN_ID DENSE_RANK DETERMINISTIC " +
    "DISTINCTROW DIV DOUBLE DO_DOMAIN_IDS DUAL EACH ELSEIF EMPTY ENCLOSED ESCAPED EXIT EXPLAIN " +
    "FIRST_VALUE FLOAT FLOAT4 FLOAT8 FORCE FULLTEXT FUNCTION GENERATED GET GROUPING GROUPS " +
    "HIGH_PRIORITY HOUR_MICROSECOND HOUR_MINUTE HOUR_SECOND IF IGNORE IGNORE_DOMAIN_IDS INFILE " +
    "INOUT INSENSITIVE INT INT1 INT2 INT3 INT4 INT8 INTEGER INTERVAL IO_AFTER_GTIDS " +
    "IO_BEFORE_GTIDS ITERATE JSON_TABLE KEYS KILL LAG LAST_VALUE LEAD LEAVE LINEAR LINES LOAD " +
    "LOCK LONG LONGBLOB LONGTEXT LOOP LOW_PRIORITY MASTER_DEMOTE_TO_REPLICA " +
    "MASTER_DEMOTE_TO_SLAVE MASTER_SSL_VERIFY_SERVER_CERT MATCH MAXVALUE MEDIUMBLOB MEDIUMINT " +
    "MEDIUMTEXT MIDDLEINT MINUTE_MICROSECOND MINUTE_SECOND MOD MODIFIES NO_WRITE_TO_BINLOG " +
    "NTH_VALUE NTILE NUMERIC OF OPTIMIZE OPTIMIZER_COSTS OPTION OPTIONALLY OUT OUTFILE OVER " +
    "PAGE_CHECKSUM PARSE_VCOL_EXPR PARTITION PERCENT_RANK PORTION PRECISION PROCEDURE PURGE RANGE " +
    "RANK READ READS READ_WRITE REAL RECURSIVE REF_SYSTEM_ID REGEXP RELEASE RENAME REPEAT REPLACE " +
    "REQUIRE RESIGNAL RESTRICT RETURN REVOKE RLIKE ROW_NUMBER SCHEMA SCHEMAS SECOND_MICROSECOND " +
    "SENSITIVE SEPARATOR SIGNAL SMALLINT SPATIAL SPECIFIC SQL SQLEXCEPTION SQLSTATE SQLWARNING " +
    "SQL_AFTER_GTIDS SQL_BEFORE_GTIDS SQL_BIG_RESULT SQL_CALC_FOUND_ROWS SQL_SMALL_RESULT SSL " +
    "STARTING STATS_AUTO_RECALC STATS_PERSISTENT STATS_SAMPLE_PAGES STORED STRAIGHT_JOIN SYSTEM " +
    "TERMINATED TINYBLOB TINYINT TINYTEXT TRIGGER UNDO UNLOCK UNSIGNED USAGE USE UTC_DATE " +
    "UTC_TIME UTC_TIMESTAMP VALUE VARBINARY VARCHAR VARCHARACTER VARYING VIRTUAL WHILE WRITE XOR " +
    "YEAR_MONTH ZEROFILL"
  ).split(" "),
);

/** Further SQLite keywords that cannot be bare names. */
export const SQLITE_RESERVED = new Set(
  (
    "ADD ALTER AUTOINCREMENT COMMIT ESCAPE IF NOTHING RAISE TRANSACTION"
  ).split(" "),
);

/** Quotes an identifier only when the dialect requires it. Mirrors `Dialect::quote_ident`
 *  in `crates/platypus-sql/src/dialect.rs`; keep the two in sync. */
export function quoteIdent(name: string, driver: DriverKind): string {
  // A field name as a mongosh document key, quoted like `shellString` (editor/mongo.ts).
  if (driver === "mongo") return /^[A-Za-z_$][\w$]*$/.test(name) ? name : JSON.stringify(name);
  const upper = name.toUpperCase();
  const reserved = RESERVED.has(upper);
  if (driver === "sqlserver") {
    const bare = /^[A-Za-z_][A-Za-z0-9_]*$/.test(name) && !reserved && !TSQL_RESERVED.has(upper);
    return bare ? name : `[${name.replaceAll("]", "]]")}]`;
  }
  if (driver === "mysql") {
    const bare = /^[A-Za-z_][A-Za-z0-9_$]*$/.test(name) && !reserved && !MYSQL_RESERVED.has(upper);
    return bare ? name : `\`${name.replaceAll("`", "``")}\``;
  }
  // A backslash escapes the next character inside ClickHouse quotes.
  if (driver === "clickhouse") {
    if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(name) && !reserved) return name;
    return `\`${name.replaceAll("`", "``").replaceAll("\\", "\\\\")}\``;
  }
  // BigQuery escapes with a backslash, Spark doubles the backtick.
  if (driver === "bigquery" || driver === "databricks") {
    if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(name) && !reserved) return name;
    const escaped =
      driver === "bigquery" ? name.replaceAll("\\", "\\\\").replaceAll("`", "\\`") : name.replaceAll("`", "``");
    return `\`${escaped}\``;
  }
  // Oracle and Db2 fold bare names to upper case; Oracle allows `$` and `#` in them.
  if (driver === "oracle" || driver === "db2") {
    const oracle = driver === "oracle";
    const bare = oracle ? /^[A-Z][A-Z0-9_$#]*$/ : /^[A-Z][A-Z0-9_]*$/;
    const bareOk = bare.test(name) && !reserved && !(oracle && ORACLE_RESERVED.has(upper));
    return bareOk ? name : `"${name.replaceAll('"', '""')}"`;
  }
  // PostgreSQL folds bare names to lower case, Snowflake to upper case and Trino to lower
  // case (only names that read the same either way stay bare); SQLite and DuckDB ignore case.
  const ansi = driver === "snowflake" || driver === "trino" || driver === "adbc" || driver === "odbc";
  const bare = driver === "postgres" ? /^[a-z_][a-z0-9_$]*$/ : ansi ? /^[A-Z_][A-Z0-9_]*$/ : /^[A-Za-z_][A-Za-z0-9_]*$/;
  const sqlite = (driver === "sqlite" || driver === "libsql") && SQLITE_RESERVED.has(upper);
  return bare.test(name) && !reserved && !sqlite ? name : `"${name.replaceAll('"', '""')}"`;
}
