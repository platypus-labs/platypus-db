/**
 * Values of `:name` and `?` written into the SQL as literals: `42`, `NULL`, `true` and
 * `'quoted'` go in as typed and anything else as a string, escaped for the driver.
 */
import type { DriverKind, ParameterSpan } from "../api";

/** A placeholder's key among the values: its name, or `?1`, `?2`… by position. */
export function parameterKeys(spans: ParameterSpan[]): string[] {
  let n = 0;
  return spans.map((p) => (p.kind === "positional" ? `?${++n}` : p.name));
}

/** Drivers whose string literals read `\` as an escape: `C:\temp\new` would hold a tab and a
 *  newline, and a value ending in `\` would leave the literal open. */
const BACKSLASH_ESCAPES = new Set<DriverKind>(["mysql", "clickhouse", "bigquery", "databricks", "snowflake"]);
/** Drivers where `''` is not a quote inside a literal (BigQuery rejects it, Spark SQL reads two
 *  literals): the quote is escaped with `\`. */
const BACKSLASH_QUOTE = new Set<DriverKind>(["bigquery", "databricks"]);

/** A value typed for `:name` or `?`, as SQL. */
export function literal(value: string, driver: DriverKind): string {
  const v = value.trim();
  if (/^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i.test(v) || /^(null|true|false)$/i.test(v) || v.startsWith("'")) {
    return v;
  }
  const escaped = BACKSLASH_ESCAPES.has(driver) ? value.replaceAll("\\", "\\\\") : value;
  const quoted = escaped.replaceAll("'", BACKSLASH_QUOTE.has(driver) ? "\\'" : "''");
  // Transact-SQL's N'...' keeps text outside the database's code page.
  const prefix = driver === "sqlserver" ? "N" : "";
  return `${prefix}'${quoted}'`;
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
  const keys = parameterKeys(spans);
  // How much longer the substituted text is so far.
  let shift = 0;
  for (const [i, p] of spans.entries()) {
    const value = values[keys[i]!] ?? "";
    const text = p.kind === "variable" ? value : literal(value, driver);
    const start = p.start + shift;
    if (pos < start) break;
    if (pos < start + text.length) return p.start;
    shift += text.length - (p.end - p.start);
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
  const keys = parameterKeys(spans);
  let out = "";
  let pos = 0;
  spans.forEach((p, i) => {
    const value = values[keys[i]!] ?? "";
    out += sql.slice(pos, p.start) + (p.kind === "variable" ? value : literal(value, driver));
    pos = p.end;
  });
  return out + sql.slice(pos);
}
