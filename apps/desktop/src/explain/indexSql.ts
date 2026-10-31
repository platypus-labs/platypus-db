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
      // The function's name, with its schema: `lower`, `app.normalize`, `"s".f`.
      const name = /(?:(?:[A-Za-z_][\w$]*|"(?:[^"]|"")+")\.)*([A-Za-z_][\w$]*)$/.exec(text.slice(0, i));
      if (!name || NOT_COLUMNS.has(name[1]!.toLowerCase())) return null;
      return [i - name[0].length, end];
    }
  }
  return null;
}

/** A cast to text that a plan adds around a column: `(name)::text`, `(o.name)::text`. */
const TEXT_CAST = /\(("(?:[^"]|"")+"|[A-Za-z_][\w$]*(?:\."(?:[^"]|"")+"|\.[A-Za-z_][\w$]*)?)\)::text/g;

/** A qualifier, `o.` or `"o".`, and what it qualifies: a call (`s.lower(`, its schema) when
 *  the second group is set, else a column (`o.email`, its table's alias). */
const QUALIFIER = /(?<![\w$."])(?:[A-Za-z_][\w$]*|"(?:[^"]|"")+")\.(?=("(?:[^"]|"")+"|[A-Za-z_][\w$]*)(\s*\()?)/g;

/** `text` with the matches of `pattern` outside its string literals replaced by `by(match)`. */
function replaceOutsideLiterals(text: string, pattern: RegExp, by: (m: RegExpMatchArray) => string): string {
  let out = "";
  let at = 0;
  // Masked literals keep their length, and what is outside them reads the same.
  for (const m of maskLiterals(text).matchAll(pattern)) {
    out += text.slice(at, m.index) + by(m);
    at = m.index + m[0].length;
  }
  return out + text.slice(at);
}

/** The expression of a call as an index key: casts to text that a plan adds around a column
 *  and the table aliases of the columns are left out, as the index has neither. Literals stay
 *  as they are, and so does the schema of a function (`s.f(col)`). */
function expressionOf(call: string): string {
  const uncast = replaceOutsideLiterals(call, TEXT_CAST, (m) => m[1]!);
  return replaceOutsideLiterals(uncast, QUALIFIER, (m) => (m[2] === undefined ? "" : m[0]));
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