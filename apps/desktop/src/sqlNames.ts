import type { DriverKind } from "./api";
import { collectionRef } from "./editor/mongo";
import { quoteIdent } from "./sqlIdent";
import { defineSetting } from "./settings";

export { quoteIdent } from "./sqlIdent";

/** Whether SQL generated for a table of the session's `current` schema may name it without
 *  its schema. Never on Trino: a session there resolves names in the schema of its
 *  connection or of a `USE`, which the navigator cannot follow, and a name it resolves
 *  elsewhere reads the rows of another table of that name. */
export function omitsSchema(driver: DriverKind, current: boolean): boolean {
  return current && driver !== "trino";
}

/** The schema generated SQL names a table of `schema` with: `null` where it is left out. */
export function schemaRef(driver: DriverKind, schema: { name: string; current: boolean }): string | null {
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

/** The data viewer's first query for a table (already quoted) and an optional `WHERE`
 *  condition. On MongoDB the table is a collection reference and the filter a query document.
 *  Once open, the editor's text is the query: `api.sqlOrderBy` sorts it. */
export function viewerSql(driver: DriverKind, table: string, filter = ""): string {
  if (driver === "mongo") return `${table}.find(${filter.trim() || "{}"})`;
  return `SELECT *\nFROM ${table}${filter.trim() ? `\nWHERE ${filter.trim()}` : ""}`;
}

/** The drag type of a table or column dragged from the navigator: `{ name, select? }`, the
 *  quoted name and, for a table, a query of it. */
export const SQL_DRAG = "application/x-platypus-sql";

export interface SqlDrag {
  name: string;
  select?: string;
}
