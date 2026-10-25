import type { DriverKind } from "./api";
import { collectionRef, shellString } from "./editor/mongo";
import { defineSetting } from "./settings";

/** Words that cannot be bare identifiers in at least one supported dialect. */
const RESERVED = new Set(
  (
    "ALL ANALYSE ANALYZE AND ANTI ANY ARRAY AS ASC ASOF ASYMMETRIC AT AUTHORIZATION BETWEEN " +
    "BINARY BOTH BY CASE CAST CHECK COLLATE COLLATION COLUMN COLUMNS CONCURRENTLY CONSTRAINT " +
    "CREATE CROSS CURRENT_DATE CURRENT_TIME CURRENT_TIMESTAMP CURRENT_USER DEFAULT " +
    "DEFERRABLE DELETE DESC DESCRIBE DISTINCT DO DROP ELSE END EXCEPT EXISTS FALSE FETCH FOR " +
    "FOREIGN FREEZE FROM FULL GLOB GRANT GROUP HAVING ILIKE IN INDEX INITIALLY INNER INSERT " +
    "INTERSECT INTO IS ISNULL JOIN KEY LAMBDA LATERAL LEADING LEFT LIKE LIMIT NATURAL NOT " +
    "NOTNULL NULL OFFSET ON ONLY OR ORDER OUTER OVERLAPS PIVOT PIVOT_LONGER PIVOT_WIDER " +
    "PLACING POSITIONAL PRIMARY QUALIFY REFERENCES RETURNING RIGHT ROW ROWS SELECT SEMI SET " +
    "SHOW SIMILAR SOME SUMMARIZE SYMMETRIC TABLE TABLESAMPLE THEN TO TRAILING TRUE UNION " +
    "UNIQUE UNPACK UNPIVOT UPDATE USER USING VALUES VARIADIC VERBOSE WHEN WHERE WINDOW WITH"
  ).split(" "),
);

/** Further words that Transact-SQL reserves (`TSQL_RESERVED` in `dialect.rs`). */
const TSQL_RESERVED = new Set(
  (
    "ADD BACKUP BEGIN BREAK BROWSE BULK CASCADE CHECKPOINT CLOSE CLUSTERED COALESCE COMMIT " +
    "COMPUTE CONTAINS CONTAINSTABLE CONTINUE CONVERT CURSOR DATABASE DBCC DEALLOCATE DECLARE " +
    "DENY DISK DISTRIBUTED DOUBLE DUMP ERRLVL ESCAPE EXEC EXECUTE EXIT EXTERNAL FILE FILLFACTOR " +
    "FREETEXT FREETEXTTABLE FUNCTION GOTO HOLDLOCK IDENTITY IDENTITYCOL IDENTITY_INSERT IF KILL " +
    "LINENO LOAD MERGE NATIONAL NOCHECK NONCLUSTERED NULLIF OF OFF OFFSETS OPEN OPENDATASOURCE " +
    "OPENQUERY OPENROWSET OPENXML OPTION OVER PERCENT PIVOT PLAN PRECISION PRINT PROC PROCEDURE " +
    "PUBLIC RAISERROR READ READTEXT RECONFIGURE REPLICATION RESTORE RESTRICT RETURN REVERT REVOKE " +
    "ROLLBACK ROWCOUNT ROWGUIDCOL RULE SAVE SCHEMA SECURITYAUDIT SEMANTICKEYPHRASETABLE " +
    "SESSION_USER SETUSER SHUTDOWN STATISTICS SYSTEM_USER TABLESAMPLE TEXTSIZE TOP TRAN " +
    "TRANSACTION TRIGGER TRUNCATE TRY_CONVERT TSEQUAL UNPIVOT UPDATETEXT USE VARYING VIEW " +
    "WAITFOR WHILE WITHIN WRITETEXT"
  ).split(" "),
);

/** Quotes an identifier only when the dialect requires it. Mirrors `Dialect::quote_ident`
 *  in `crates/platypus-sql/src/dialect.rs`; keep the two in sync. */
export function quoteIdent(name: string, driver: DriverKind): string {
  // A field name as a mongosh document key.
  if (driver === "mongo") return /^[A-Za-z_$][\w$]*$/.test(name) ? name : shellString(name);
  const upper = name.toUpperCase();
  const reserved = RESERVED.has(upper);
  if (driver === "sqlserver") {
    const bare = /^[A-Za-z_][A-Za-z0-9_]*$/.test(name) && !reserved && !TSQL_RESERVED.has(upper);
    return bare ? name : `[${name.replaceAll("]", "]]")}]`;
  }
  if (driver === "mysql") {
    return /^[A-Za-z_][A-Za-z0-9_$]*$/.test(name) && !reserved ? name : `\`${name.replaceAll("`", "``")}\``;
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
  // PostgreSQL folds bare names to lower case, Snowflake to upper case and Trino to lower
  // case (only names that read the same either way stay bare); SQLite and DuckDB ignore case.
  const ansi = driver === "snowflake" || driver === "trino" || driver === "adbc";
  const bare = driver === "postgres" ? /^[a-z_][a-z0-9_$]*$/ : ansi ? /^[A-Z_][A-Z0-9_]*$/ : /^[A-Za-z_][A-Za-z0-9_]*$/;
  return bare.test(name) && !reserved ? name : `"${name.replaceAll('"', '""')}"`;
}

/** `schema.name`, or just `name` for the session's current schema. */
export function qualifiedName(
  driver: DriverKind,
  schema: { name: string; current: boolean },
  name: string,
): string {
  if (driver === "mongo") return collectionRef(schema.current ? null : schema.name, name);
  const table = quoteIdent(name, driver);
  return schema.current ? table : `${quoteIdent(schema.name, driver)}.${table}`;
}

export const selectLimit = defineSetting({
  key: "navigator.select_limit",
  section: "Navigator",
  label: "LIMIT of Generate SELECT",
  default: 200,
  control: { type: "number", min: 1, step: 100 },
});

export function selectSql(
  driver: DriverKind,
  target: string,
  columns: string[] | null,
  limit: number | null,
): string {
  if (driver === "mongo") {
    const projection = columns?.length ? `, { ${columns.map((c) => `${quoteIdent(c, driver)}: 1`).join(", ")} }` : "";
    return `${target}.find({}${projection})${limit === null ? "" : `.limit(${limit})`};`;
  }
  const list = columns?.length ? columns.map((c) => quoteIdent(c, driver)).join(",\n       ") : "*";
  // Transact-SQL has no LIMIT: `SELECT TOP (n)`, as `Dialect::limited` writes it.
  if (driver === "sqlserver" && limit !== null) return `SELECT TOP (${limit}) ${list}\nFROM ${target};`;
  return `SELECT ${list}\nFROM ${target}${limit === null ? "" : `\nLIMIT ${limit}`};`;
}

export interface Sort {
  column: string;
  desc: boolean;
}

/** The data viewer's query for a table (already quoted), a `WHERE` filter and a sort. On
 *  MongoDB the table is a collection reference and the filter a query document. */
export function viewerSql(driver: DriverKind, table: string, filter: string, sort: Sort | null): string {
  if (driver === "mongo") {
    const order = sort ? `.sort({ ${quoteIdent(sort.column, driver)}: ${sort.desc ? -1 : 1} })` : "";
    return `${table}.find(${filter.trim() || "{}"})${order}`;
  }
  let sql = `SELECT *\nFROM ${table}`;
  if (filter.trim()) sql += `\nWHERE ${filter.trim()}`;
  if (sort) sql += `\nORDER BY ${quoteIdent(sort.column, driver)} ${sort.desc ? "DESC" : "ASC"}`;
  return sql;
}

/** The drag type of a table or column dragged from the navigator: `{ name, select? }`, the
 *  quoted name and, for a table, a query of it. */
export const SQL_DRAG = "application/x-platypus-sql";

export interface SqlDrag {
  name: string;
  select?: string;
}
