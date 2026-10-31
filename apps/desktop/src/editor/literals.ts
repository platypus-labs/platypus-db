/**
 * Values of `:name` and `?` written into the SQL as literals: `42`, `NULL`, `true` and
 * `'quoted'` go in as typed (a negative number in parentheses) and anything else, codes such as
 * `007` included, as a string, escaped for the driver.
 */
import type { DriverKind, ParameterSpan } from "../api";

/** A placeholder's key among the values: its name, or `?1`, `?2`… by position. As in SQLite, a
 *  plain `?` after a numbered `?3` is `?4`. */
export function parameterKeys(spans: ParameterSpan[]): string[] {
  let n = 0;
  return spans.map((p) => {
    if (p.kind === "positional") return `?${++n}`;
    const numbered = /^\?(\d+)$/.exec(p.name);
    if (numbered) n = Math.max(n, Number(numbered[1]));
    return p.name;
  });
}

/** Drivers whose string literals read `\` as an escape: `C:\temp\new` would hold a tab and a
 *  newline, and a value ending in `\` would leave the literal open. */
const BACKSLASH_ESCAPES = new Set<DriverKind>(["mysql", "clickhouse", "bigquery", "databricks", "snowflake"]);
/** Drivers where `''` is not a quote inside a literal (BigQuery rejects it, Spark SQL reads two
 *  literals): the quote is escaped with `\`. */
const BACKSLASH_QUOTE = new Set<DriverKind>(["bigquery", "databricks"]);

/** Whether `v` is a number that reads back as typed: not a code such as `007`, `0042` or
 *  `+39333…`, which a number would turn into another value. */
function isNumber(v: string): boolean {
  return /^-?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i.test(v) && !/^-?0\d/.test(v);
}

/** A value typed for `:name` or `?`, as SQL. A negative number goes in parentheses: after a
 *  `-` it would start a `--` comment, and before `::type` the cast would take only its digits. */
export function literal(value: string, driver: DriverKind): string {
  const v = value.trim();
  if (isNumber(v)) return v.startsWith("-") ? `(${v})` : v;
  if (/^(null|true|false)$/i.test(v) || v.startsWith("'")) return v;
  const escaped = BACKSLASH_ESCAPES.has(driver) ? value.replaceAll("\\", "\\\\") : value;
  const quoted = escaped.replaceAll("'", BACKSLASH_QUOTE.has(driver) ? "\\'" : "''");
  // Transact-SQL's N'...' keeps text outside the database's code page.
  const prefix = driver === "sqlserver" ? "N" : "";
  return `${prefix}'${quoted}'`;
}

/** Whether characters `a` and `b` side by side start a comment (`--` or `/*`). */
const opensComment = (a: string | undefined, b: string | undefined) => (a === "-" && b === "-") || (a === "/" && b === "*");

/** `sql` with its placeholders replaced by `values`, and where each value went: `at` is its
 *  range in the text, `from`/`to` its placeholder's in `sql`. A value never touches the text
 *  around it so as to open a comment: `10-${v}` with `-5` becomes `10- -5`. */
function fill(sql: string, spans: ParameterSpan[], values: Record<string, string>, driver: DriverKind) {
  const keys = parameterKeys(spans);
  const ranges: { start: number; end: number; from: number; to: number }[] = [];
  let text = "";
  let pos = 0;
  spans.forEach((p, i) => {
    const value = values[keys[i]!] ?? "";
    let piece = p.kind === "variable" ? value : literal(value, driver);
    text += sql.slice(pos, p.start);
    if (opensComment(text.at(-1), piece[0])) piece = ` ${piece}`;
    if (opensComment(piece.at(-1), sql[p.end])) piece = `${piece} `;
    ranges.push({ start: text.length, end: text.length + piece.length, from: p.start, to: p.end });
    text += piece;
    pos = p.end;
  });
  return { text: text + sql.slice(pos), ranges };
}

/** Where offset `pos` of `substitute(sql, spans, values, driver)` stands in `sql`: past a value,
 *  shifted back by what the values added; inside one, at its placeholder. */
export function originalOffset(
  sql: string,
  spans: ParameterSpan[],
  values: Record<string, string>,
  driver: DriverKind,
  pos: number,
): number {
  // How much longer the substituted text is so far.
  let shift = 0;
  for (const r of fill(sql, spans, values, driver).ranges) {
    if (pos < r.start) break;
    if (pos < r.end) return r.from;
    shift = r.end - r.to;
  }
  return Math.max(0, Math.min(sql.length, pos - shift));
}

/** The UTF-16 offset (JavaScript's) of the code point at index `n` of `text`: servers count
 *  characters, so an emoji before an error moves it by one unit. */
export function utf16Offset(text: string, n: number): number {
  let units = 0;
  let count = 0;
  for (const ch of text) {
    if (count >= n) break;
    units += ch.length;
    count++;
  }
  return units + Math.max(0, n - count);
}

/** `sql` with its placeholders (offsets relative to `sql`) replaced by `values`. */
export function substitute(
  sql: string,
  spans: ParameterSpan[],
  values: Record<string, string>,
  driver: DriverKind,
): string {
  return fill(sql, spans, values, driver).text;
}
