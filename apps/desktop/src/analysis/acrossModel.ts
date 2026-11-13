/**
 * "Query across databases": the pure part. The names of the DuckDB tables the sources become,
 * what is wrong with them, and the text the new tab starts with. The unit tests import it.
 */
import { quoteIdent } from "../sqlIdent.ts";

/** Where a table comes from, kept with the tab for "Reload sources". */
export interface AcrossSourceRef {
  connectionId: string;
  connectionName: string;
  database: string | null;
  /** Empty for the session's current schema. */
  schema: string;
  table: string;
  /** The condition of a `WHERE`, without the keyword; empty for every row. */
  filter: string;
  /** The table's name in the DuckDB. */
  name: string;
}

/** The sources of an "Across databases" tab and the tables that lack rows. */
export interface AcrossOrigin {
  sources: AcrossSourceRef[];
  /** Names of the tables the memory limit cut. */
  cut: string[];
}

/** A table of the DuckDB as the command reports it. */
export interface AcrossLoaded {
  name: string;
  columns: string[];
  rows: number;
  cut: boolean;
}

/** `<connection>_<table>` in lower case, anything but letters and digits made `_`. */
export function defaultName(connectionName: string, table: string): string {
  const slug = (s: string) =>
    s
      .toLowerCase()
      .replace(/[^\p{L}\p{N}]+/gu, "_")
      .replace(/^_+|_+$/g, "");
  return [slug(connectionName), slug(table)].filter((s) => s !== "").join("_") || "table";
}

/** `base`, or `base_2`, `base_3`… the first not in `taken` (compared ignoring case, as DuckDB
 *  does); the result is added to `taken`. */
export function uniqueName(base: string, taken: Set<string>): string {
  let name = base;
  let n = 2;
  while (taken.has(name.toLowerCase())) name = `${base}_${n++}`;
  taken.add(name.toLowerCase());
  return name;
}

/** A row of the dialog as far as naming goes. */
interface NamedRow {
  connectionName: string;
  table: string;
  /** The user typed the name: it stays as typed. */
  edited: boolean;
  name: string;
}

/** The name of each row: typed names as typed, the others the default made unique among them. */
export function assignNames(rows: readonly NamedRow[]): string[] {
  const taken = new Set(rows.filter((r) => r.edited).map((r) => r.name.trim().toLowerCase()));
  return rows.map((r) => (r.edited ? r.name : uniqueName(defaultName(r.connectionName, r.table), taken)));
}

export type NameProblem = "empty" | "duplicate";

/** What is wrong with each name: empty, or taken by an earlier row (ignoring case). */
export function nameProblems(names: readonly string[]): (NameProblem | null)[] {
  const seen = new Set<string>();
  return names.map((raw) => {
    const name = raw.trim().toLowerCase();
    if (name === "") return "empty";
    if (seen.has(name)) return "duplicate";
    seen.add(name);
    return null;
  });
}

/** The tab's temporary connection name: `Across: pg_orders, my_customers`. */
export function connectionTitle(names: readonly string[]): string {
  return `Across: ${names.join(", ")}`;
}

/** The editor's first text: the tables and their columns in a comment, then a query of the
 *  first. */
export function initialSql(tables: readonly AcrossLoaded[]): string {
  const lines = tables.map((t) => `--   ${t.name}(${t.columns.join(", ")})`);
  const first = tables[0]?.name ?? "";
  const quoted = quoteIdent(first, "duckdb");
  return `-- Tables, loaded from other databases:\n${lines.join("\n")}\nSELECT * FROM ${quoted} LIMIT 100`;
}
