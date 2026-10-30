import type { DriverKind } from "../api";
import { quoteIdent } from "../sqlIdent.ts";

/** Words that may stand before an operator in a filter without being a column. */
const NOT_COLUMNS = new Set(["and", "or", "not", "null", "true", "false", "any", "all", "some", "in", "is", "like", "ilike", "between", "exists", "case", "when", "then", "else", "end", "cast", "as"]);

/** An identifier (bare or quoted, optionally behind `alias.`, optionally cast) followed by a
 *  comparison: `status = ...`, `("total")::numeric > ...`, `t.name LIKE ...`, `x IS NULL`. */
const COMPARED = /(?:[A-Za-z_][\w$]*\.)?("(?:[^"]|"")+"|`[^`]+`|\[[^\]]+\]|[A-Za-z_][\w$]*)\)?(?:::[\w. ]+?)?\s*(?:=|<>|!=|<=|>=|<|>|~~\*?|!~~\*?|~\*?|\bnot\s+like\b|\blike\b|\bilike\b|\bin\b|\bis\b|\bbetween\b)/gi;

/** The columns a plan's filter condition compares, in order of appearance and without
 *  repeats; empty when none can be read. Literals are not looked into. */
export function filterColumns(filter: string): string[] {
  const bare = filter.replace(/'(?:[^']|'')*'/g, "''");
  const columns: string[] = [];
  for (const m of bare.matchAll(COMPARED)) {
    let name = m[1]!;
    if (/^["`[]/.test(name)) name = name.slice(1, -1).replace(/""/g, '"');
    else if (NOT_COLUMNS.has(name.toLowerCase())) continue;
    if (!columns.includes(name)) columns.push(name);
  }
  return columns;
}

/** `CREATE INDEX` on `table` for `columns`, named `<table>_<columns>_idx`; the table may be
 *  qualified (`schema.name`), as some plans show it. */
export function createIndexSql(driver: DriverKind, table: string, columns: string[]): string {
  const parts = table.includes(".") ? table.split(".") : [table];
  const bare = parts[parts.length - 1]!;
  const name = `${bare}_${columns.join("_")}_idx`.replace(/[^\w$]/g, "_").slice(0, 63);
  const target = parts.map((p) => quoteIdent(p, driver)).join(".");
  return `CREATE INDEX ${quoteIdent(name, driver)} ON ${target} (${columns.map((c) => quoteIdent(c, driver)).join(", ")});`;
}
