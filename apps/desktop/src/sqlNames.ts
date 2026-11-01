import type { DriverKind } from "./api";
import { collectionRef } from "./editor/mongo";
import { searchRequest } from "./searchSql";
import { quoteIdent, redisArg } from "./sqlIdent";
import { defineSetting } from "./settings";

export { keyOrderClause, quoteIdent } from "./sqlIdent";

/** Whether SQL generated for a table of the session's `current` schema may name it without
 *  its schema. Never on Trino: a session there resolves names in the schema of its
 *  connection or of a `USE`, which the navigator cannot follow, and a name it resolves
 *  elsewhere reads the rows of another table of that name. Nor on Redis, whose `SELECT` in
 *  the editor moves the session to another database. */
export function omitsSchema(driver: DriverKind, current: boolean): boolean {
  return current && driver !== "trino" && driver !== "redis";
}

/** The schema generated SQL names a table of `schema` with: `null` where it is left out. */
export function schemaRef(driver: DriverKind, schema: { name: string; current: boolean }): string | null {
  // The one schema of a cluster is its name, which no request names.
  if (driver === "elasticsearch") return null;
  return omitsSchema(driver, schema.current) ? null : schema.name;
}

/** `schema.name`, or just `name` for the session's current schema. */
export function qualifiedName(
  driver: DriverKind,
  schema: { name: string; current: boolean },
  name: string,
): string {
  if (driver === "mongo") return collectionRef(schema.current ? null : schema.name, name);
  const table = quoteIdent(name, driver);
  const own = schemaRef(driver, schema);
  return own === null ? table : `${quoteIdent(own, driver)}.${table}`;
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
  if (driver === "redis") return `BROWSE ${target}${limit === null ? "" : ` LIMIT ${limit}`}`;
  if (driver === "elasticsearch") return searchRequest(target, limit);
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
  /** The column's place in the select list (from 1), when its name cannot point to it. */
  position?: number;
}

/** The data viewer's first query for a table (already quoted) and an optional `WHERE`
 *  condition. On MongoDB the table is a collection reference and the filter a query document;
 *  on Redis the table is a key type and the filter a pattern of the keys (`user:*`); on
 *  Elasticsearch the table is an index and the filter a query string. `hiddenKey` is a column
 *  `SELECT *` leaves out that the rows are edited by (CockroachDB's `rowid`): it is selected too.
 *  Once open, the editor's text is the query: `api.sqlOrderBy` sorts it. */
export function viewerSql(driver: DriverKind, table: string, filter = "", hiddenKey: string | null = null): string {
  if (driver === "mongo") return `${table}.find(${filter.trim() || "{}"})`;
  if (driver === "redis") return `BROWSE ${table}${filter.trim() ? ` MATCH ${redisArg(filter.trim())}` : ""}`;
  // The filter is a query string, `status:active AND price:[10 TO 20]`.
  if (driver === "elasticsearch") return searchRequest(table, null, filter);
  const list = hiddenKey ? `*, ${quoteIdent(hiddenKey, driver)}` : "*";
  return `SELECT ${list}\nFROM ${table}${filter.trim() ? `\nWHERE ${filter.trim()}` : ""}`;
}

/** A table or column dragged from the navigator: the quoted name and, for a table, a
 *  query of it. */
export interface SqlDrag {
  name: string;
  select?: string;
}
