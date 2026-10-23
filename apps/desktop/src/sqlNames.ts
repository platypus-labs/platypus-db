import type { DriverKind } from "./api";

/** Words that cannot be bare identifiers in at least one supported dialect. */
const RESERVED = new Set(
  (
    "ALL AND ANY ARRAY AS ASC BETWEEN BOTH BY CASE CAST CHECK COLLATE COLUMN CONSTRAINT CREATE " +
    "CROSS CURRENT_DATE CURRENT_TIME CURRENT_TIMESTAMP CURRENT_USER DEFAULT DELETE DESC " +
    "DISTINCT DO DROP ELSE END EXCEPT EXISTS FALSE FETCH FOR FOREIGN FROM FULL GRANT GROUP " +
    "HAVING IN INDEX INNER INSERT INTERSECT INTO IS JOIN KEY LEADING LEFT LIKE LIMIT NATURAL " +
    "NOT NULL OFFSET ON OR ORDER OUTER PRIMARY REFERENCES RIGHT ROW ROWS SELECT SET SOME " +
    "TABLE THEN TO TRAILING TRUE UNION UNIQUE UPDATE USER USING VALUES WHEN WHERE WINDOW WITH"
  ).split(" "),
);

/** Quotes an identifier only when the dialect requires it. */
export function quoteIdent(name: string, driver: DriverKind): string {
  const reserved = RESERVED.has(name.toUpperCase());
  if (driver === "mysql") {
    return /^[A-Za-z_][A-Za-z0-9_$]*$/.test(name) && !reserved ? name : `\`${name.replaceAll("`", "``")}\``;
  }
  // PostgreSQL folds bare names to lower case; SQLite ignores case.
  const bare = driver === "postgres" ? /^[a-z_][a-z0-9_$]*$/ : /^[A-Za-z_][A-Za-z0-9_]*$/;
  return bare.test(name) && !reserved ? name : `"${name.replaceAll('"', '""')}"`;
}

/** `schema.name`, or just `name` for the session's current schema. */
export function qualifiedName(
  driver: DriverKind,
  schema: { name: string; current: boolean },
  name: string,
): string {
  const table = quoteIdent(name, driver);
  return schema.current ? table : `${quoteIdent(schema.name, driver)}.${table}`;
}

/** Rows shown by "Open data". */
export const OPEN_DATA_LIMIT = 200;

export function selectSql(
  driver: DriverKind,
  target: string,
  columns: string[] | null,
  limit: number | null,
): string {
  const list = columns?.length ? columns.map((c) => quoteIdent(c, driver)).join(",\n       ") : "*";
  return `SELECT ${list}\nFROM ${target}${limit === null ? "" : `\nLIMIT ${limit}`};`;
}
