/**
 * Parameters and client commands of the SQL editor. `:name` and `?` take a literal, so
 * `42`, `NULL`, `true` and `'quoted'` go in as typed and anything else as a string;
 * `${name}` takes the text as typed, e.g. a table name. Values are the tab's variables,
 * which `@set name = value` also sets.
 */
import type { DriverKind, ExportFormat, ParameterSpan } from "../api";
import { t } from "../i18n";

/** A placeholder's key among the values: its name, or `?1`, `?2`… by position. */
export function parameterKeys(spans: ParameterSpan[]): string[] {
  let n = 0;
  return spans.map((p) => (p.kind === "positional" ? `?${++n}` : p.name));
}

/** A value typed for `:name` or `?`, as SQL. */
export function literal(value: string, driver: DriverKind): string {
  const v = value.trim();
  if (/^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i.test(v) || /^(null|true|false)$/i.test(v) || v.startsWith("'")) {
    return v;
  }
  const escaped = driver === "mysql" ? value.replaceAll("\\", "\\\\") : value;
  // Transact-SQL's N'...' keeps text outside the database's code page.
  const prefix = driver === "sqlserver" ? "N" : "";
  return `${prefix}'${escaped.replaceAll("'", "''")}'`;
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

/** `${name}` in a client command's text, replaced as typed; unknown names stay. */
export function expandVariables(text: string, values: Record<string, string>): string {
  return text.replace(/\$\{([\w.]+)\}/g, (all, name: string) => values[name] ?? all);
}

export type ClientCommand =
  | { name: "set"; variable: string; value: string }
  | { name: "echo"; text: string }
  | { name: "include"; path: string }
  | { name: "export"; path: string; format: ExportFormat }
  | { name: "error"; message: string };

const unquote = (s: string) => s.trim().replace(/^(['"])(.*)\1$/, "$2");

const FORMATS: Record<string, ExportFormat> = {
  csv: "csv",
  json: "json",
  ndjson: "ndjson",
  parquet: "parquet",
  xlsx: "xlsx",
  sql: "sql",
  md: "markdown",
  html: "html",
  htm: "html",
  xml: "xml",
};

/** Reads `@set x = 1`, `@echo text`, `@include file.sql` or `@export file.csv`. */
export function parseCommand(text: string): ClientCommand {
  const m = /^@(\w+)\s*(.*)$/s.exec(text.trim());
  const [, word = "", rest = ""] = m ?? [];
  switch (word.toLowerCase()) {
    case "set": {
      const set = /^([\w.]+)\s*(?:=\s*|\s)(.*)$/s.exec(rest);
      return set
        ? { name: "set", variable: set[1]!, value: set[2]!.trim() }
        : { name: "error", message: t("Usage: @set name = value") };
    }
    case "echo":
      return { name: "echo", text: rest };
    case "include":
      return rest.trim() ? { name: "include", path: unquote(rest) } : { name: "error", message: t("Usage: @include file.sql") };
    case "export": {
      const path = unquote(rest);
      if (!path) return { name: "error", message: t("Usage: @export file.csv (also .json, .ndjson, .parquet, .xlsx, .sql, .md, .html, .xml)") };
      const ext = /\.(\w+)$/.exec(path)?.[1]?.toLowerCase() ?? "";
      return { name: "export", path, format: FORMATS[ext] ?? "csv" };
    }
    default:
      return { name: "error", message: t("Unknown command @{word}: use @set, @echo, @include or @export", { word }) };
  }
}

/** `path` against the folder of `base` (the tab's file); absolute paths stay. */
export function resolvePath(path: string, base: string | null): string | null {
  if (/^([a-zA-Z]:[\\/]|[\\/])/.test(path)) return path;
  if (!base) return null;
  const dir = base.replace(/[\\/][^\\/]*$/, "");
  return `${dir}${base.includes("\\") ? "\\" : "/"}${path}`;
}
