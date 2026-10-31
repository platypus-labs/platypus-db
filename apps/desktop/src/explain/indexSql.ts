import type { DriverKind } from "../api";
import { quoteIdent } from "../sqlIdent.ts";

/** Words that may stand before an operator in a filter without being a column. */
const NOT_COLUMNS = new Set(["and", "or", "not", "null", "true", "false", "any", "all", "some", "in", "is", "like", "ilike", "between", "exists", "case", "when", "then", "else", "end", "cast", "as"]);

/** An identifier (bare or quoted, optionally behind `alias.`, optionally cast; never the type
 *  after `::` nor a part of a longer name) followed by a
 *  comparison: `status = ...`, `("total")::numeric > ...`, `t.name LIKE ...`, `x IS NULL`. */
const COMPARED = /(?<![\w$:."\]`])(?:(?:[A-Za-z_][\w$]*|"(?:[^"]|"")+"|`[^`]+`)\.)?("(?:[^"]|"")+"|`[^`]+`|\[[^\]]+\]|[A-Za-z_][\w$]*)\)?(?:::[\w. ]+?)?\)?\s*(?:=|<>|!=|<=|>=|<|>|~~\*?|!~~\*?|~\*?|\bnot\s+like\b|\blike\b|\bilike\b|\bin\b|\bis\b|\bbetween\b)/gi;

/** A part of the key an index is suggested on: a column, or an expression (`lower(email)`)
 *  that a condition compares, which only an index on the same expression serves. */
export type IndexKey = string | { expression: string };

/** Comparison operators and keywords, the places where an operand ends. */
const OPERATORS = /<>|!=|<=|>=|=|<|>|!~~\*?|~~\*?|~\*?|\bnot\s+like\b|\blike\b|\bilike\b|\bin\b|\bis\b|\bbetween\b/gi;

/** `text` with every string literal kept in length but emptied, so positions agree. */
const maskLiterals = (text: string) => text.replace(/'(?:[^']|'')*'/g, (m) => `'${"_".repeat(m.length - 2)}'`);

/** The function call that ends at `end` (exclusive) in `text`, as `[start, end]`; `null` when
 *  what ends there is not a call (a parenthesised column, a number...). */
function callBefore(text: string, end: number): [number, number] | null {
  if (text[end - 1] !== ")") return null;
  let depth = 0;
  for (let i = end - 1; i >= 0; i--) {
    if (text[i] === ")") depth++;
    else if (text[i] === "(" && --depth === 0) {
      const name = /[A-Za-z_][\w$]*$/.exec(text.slice(0, i));
      if (!name || NOT_COLUMNS.has(name[0].toLowerCase())) return null;
      return [i - name[0].length, end];
    }
  }
  return null;
}

/** The expression of a call as an index key: casts to text that a plan adds around a column
 *  and the aliases of the columns are left out, as the index has neither. */
function expressionOf(call: string): string {
  return call
    .replace(/\(("(?:[^"]|"")+"|[A-Za-z_][\w$]*(?:\."(?:[^"]|"")+"|\.[A-Za-z_][\w$]*)?)\)::text/g, "$1")
    .replace(/(?<![\w$."])(?:[A-Za-z_][\w$]*|"(?:[^"]|"")+")\.(?=["A-Za-z_])/g, "");
}

/** What a plan's filter condition compares, in order of appearance and without repeats: the
 *  columns, and the function calls (`lower(email) = ...`) that are compared whole. Empty when
 *  none can be read. Literals are not looked into. */
export function filterKeys(filter: string): IndexKey[] {
  const bare = maskLiterals(filter);
  const found: { at: number; key: IndexKey }[] = [];
  const blanked = [...bare];
  for (const op of bare.matchAll(OPERATORS)) {
    let end = op.index;
    while (end > 0 && /\s/.test(bare[end - 1]!)) end--;
    // A cast after the call: `lower(x)::text = ...`.
    const cast = /::[\w. ]+$/.exec(bare.slice(0, end));
    if (cast) end -= cast[0].length;
    const call = callBefore(bare, end);
    if (!call) continue;
    const expression = expressionOf(filter.slice(call[0], call[1]));
    if (!found.some((f) => typeof f.key !== "string" && f.key.expression === expression)) found.push({ at: call[0], key: { expression } });
    blanked.fill(" ", call[0], call[1]);
  }
  const rest = blanked.join("");
  for (const m of rest.matchAll(COMPARED)) {
    let name = m[1]!;
    if (/^["`[]/.test(name)) name = name.slice(1, -1).replace(/""/g, '"');
    else if (NOT_COLUMNS.has(name.toLowerCase())) continue;
    if (!found.some((f) => f.key === name)) found.push({ at: m.index, key: name });
  }
  return found.sort((a, b) => a.at - b.at).map((f) => f.key);
}

/** The words of an index key in the index's name. */
const keyWords = (key: IndexKey) => (typeof key === "string" ? key : key.expression);

/** `CREATE INDEX` on `table` for `keys`, named `<table>_<keys>_idx`; the table may be
 *  qualified (`schema.name`), as some plans show it. */
export function createIndexSql(driver: DriverKind, table: string, keys: IndexKey[]): string {
  const parts = table.includes(".") ? table.split(".") : [table];
  const bare = parts[parts.length - 1]!;
  const words = keys.map(keyWords).join("_").replace(/[^\w$]+/g, "_").replace(/^_+|_+$/g, "");
  const name = `${bare}_${words}_idx`.replace(/[^\w$]/g, "_").slice(0, 63);
  const target = parts.map((p) => quoteIdent(p, driver)).join(".");
  const list = keys.map((k) => (typeof k === "string" ? quoteIdent(k, driver) : `(${k.expression})`));
  return `CREATE INDEX ${quoteIdent(name, driver)} ON ${target} (${list.join(", ")});`;
}