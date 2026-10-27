/**
 * Parameters and client commands of the SQL editor. `:name` and `?` take a literal, so
 * `42`, `NULL`, `true` and `'quoted'` go in as typed and anything else as a string;
 * `${name}` takes the text as typed, e.g. a table name. Values are the tab's variables,
 * which `@set name = value` also sets.
 */
import type { Compression, ExportFormat } from "../api";
import { t } from "../i18n";

export { literal, parameterKeys, substitute } from "./literals";

/** `${name}` in a client command's text, replaced as typed; unknown names stay. */
export function expandVariables(text: string, values: Record<string, string>): string {
  return text.replace(/\$\{([\w.]+)\}/g, (all, name: string) => values[name] ?? all);
}

export type ClientCommand =
  | { name: "set"; variable: string; value: string }
  | { name: "echo"; text: string }
  | { name: "include"; path: string }
  | { name: "export"; path: string; format: ExportFormat; delimiter: string; compression: Compression }
  | { name: "error"; message: string };

/** Where `@export` sends the next statement's rows, and how. */
export type ExportTarget = Omit<Extract<ClientCommand, { name: "export" }>, "name">;

const unquote = (s: string) => s.trim().replace(/^(['"])(.*)\1$/, "$2");

const FORMATS: Record<string, ExportFormat> = {
  csv: "csv",
  tsv: "csv",
  txt: "csv",
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
      const usage = t("Usage: @export file.csv (also .tsv, .json, .ndjson, .parquet, .xlsx, .sql, .md, .html, .xml, and .gz or .zst after them)");
      if (!path) return { name: "error", message: usage };
      // `.gz` and `.zst` compress the file of the extension before them.
      const [, base = path, packed] = /^(.*?)(?:\.(gz|zst))?$/i.exec(path) ?? [];
      const compression: Compression = packed?.toLowerCase() === "gz" ? "gzip" : packed ? "zstd" : "none";
      const ext = /\.(\w+)$/.exec(base)?.[1]?.toLowerCase();
      // A file without an extension is CSV; one the export does not know is an error.
      const format = ext === undefined ? "csv" : FORMATS[ext];
      if (!format) return { name: "error", message: usage };
      return { name: "export", path, format, delimiter: ext === "tsv" ? "\t" : ",", compression };
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
