/** What the data editor needs to know about the table behind a result. */
import type { ColumnInfo, ColumnMeta, ObjectKind, SourceItem, TableDetails, VirtualKey } from "../../api";

/** The table a data viewer browses. */
export interface TableRef {
  schema: string;
  name: string;
  kind: ObjectKind;
}

/** The table behind a result, as the editor loads it. */
export interface EditSource {
  table: TableRef;
  details: TableDetails;
  /** The query's select list; `null` when the result has exactly the table's columns. */
  items: SourceItem[] | null;
}

export interface EditMeta {
  table: TableRef;
  details: TableDetails;
  /** Result columns that identify a row. */
  key: number[];
  /** The virtual key's columns, when the key is one the user set. */
  virtualKey: string[] | null;
  /** Catalog details by result column; `undefined` for expressions, which stay read-only. */
  columns: (ColumnMeta | undefined)[];
}

/** Why a result cannot be edited; `noKey` when a virtual key would make it editable. */
export interface ReadOnly {
  reason: string;
  /** Not worth saying, e.g. a query in the SQL editor that joins tables. */
  hidden?: boolean;
  noKey?: { table: TableRef; columns: string[] };
}

/** How a column is edited. */
export type EditorKind = "text" | "multiline" | "bool" | "enum" | "date" | "lookup";

/** Finds a catalog column by name: exactly, else ignoring case (MySQL and SQLite). */
function byName(details: TableDetails): (name: string) => ColumnMeta | undefined {
  const exact = new Map(details.columns.map((c) => [c.name, c]));
  const folded = new Map(details.columns.map((c) => [c.name.toLowerCase(), c]));
  return (name) => exact.get(name) ?? folded.get(name.toLowerCase());
}

/** The table column behind each result column, by the select list; a string says why the
 *  columns cannot be matched. */
export function resultColumns(source: EditSource, columns: ColumnInfo[]): (ColumnMeta | undefined)[] | string {
  const find = byName(source.details);
  const expected: (string | null)[] = source.items
    ? source.items.flatMap((item) =>
        item.kind === "all" ? source.details.columns.map((c) => c.name) : [item.kind === "column" ? item.name : null],
      )
    : columns.map((c) => c.name);
  if (expected.length !== columns.length) return "its columns do not match the query";
  const seen = new Set<ColumnMeta>();
  return columns.map((column, i) => {
    const name = expected[i];
    const meta = name == null ? undefined : find(name);
    // A column shown twice is edited through its first appearance only.
    if (!meta || find(column.name) !== meta || seen.has(meta)) return undefined;
    seen.add(meta);
    return meta;
  });
}

/**
 * The key to edit `columns` by: the primary key, else a unique index on non-null columns,
 * else the virtual key the user set. Returns why the result is read-only when there is none.
 */
export function editMeta(source: EditSource, columns: ColumnInfo[], virtualKeys: VirtualKey[]): EditMeta | ReadOnly {
  const { table, details } = source;
  if (table.kind !== "table") return { reason: "only tables can be edited" };
  const mapped = resultColumns(source, columns);
  if (typeof mapped === "string") return { reason: mapped };
  if (mapped.every((c) => !c)) return { reason: "the result has none of the table's columns" };
  const index = new Map(mapped.flatMap((c, i) => (c ? [[c.name, i] as const] : [])));
  const present = (names: string[]) => names.length > 0 && names.every((n) => index.has(n));
  const find = byName(details);
  // SQLite reports `INTEGER PRIMARY KEY` columns as nullable, though they never hold NULL.
  const primary =
    details.indexes.find((i) => i.primary)?.columns ??
    details.columns.filter((c) => c.primary_key).map((c) => c.name);
  const uniques = details.indexes.filter((i) => i.unique && i.columns.every((n) => find(n)?.nullable === false));
  const unique = uniques.find((i) => present(i.columns));
  const virtual = virtualKeys
    .find((k) => k.schema === table.schema && k.table === table.name)
    ?.columns.map((n) => find(n)?.name ?? n);
  const meta = (key: string[], virtualKey: string[] | null): EditMeta => ({
    table,
    details,
    key: key.map((n) => index.get(n)!),
    virtualKey,
    columns: mapped,
  });
  if (present(primary)) return meta(primary, null);
  if (unique) return meta(unique.columns, null);
  if (virtual && present(virtual)) return meta(virtual, virtual);
  const keyed = primary.length > 0 || uniques.length > 0 || virtual !== undefined;
  if (keyed) return { reason: "the result does not include the table's key columns" };
  return {
    reason: "the table has no primary key or unique key on non-null columns",
    noKey: { table, columns: details.columns.map((c) => c.name) },
  };
}

const JSON_TYPE = /json/i;
const BOOL_TYPE = /^bool(ean)?$/i;
const DATE = /^\d{4}-\d{2}-\d{2}$/;
/** Longer values get the multi-line editor. */
const LONG_TEXT = 120;

export function editorKind(
  column: ColumnInfo,
  meta: ColumnMeta | undefined,
  value: string | null,
  lookup = false,
): EditorKind {
  const type = meta?.data_type ?? column.db_type;
  if (lookup) return "lookup";
  if (meta?.enum_values.length) return "enum";
  // Booleans stored as numbers (SQLite, MySQL TINYINT) are typed as 1 and 0.
  if (BOOL_TYPE.test(type) && !column.numeric) return "bool";
  if (JSON_TYPE.test(type) || (value !== null && (value.includes("\n") || value.length > LONG_TEXT))) {
    return "multiline";
  }
  if (/^date$/i.test(type) && (value === null || DATE.test(value))) return "date";
  return "text";
}

export function isJson(column: ColumnInfo, meta: ColumnMeta | undefined): boolean {
  return JSON_TYPE.test(meta?.data_type ?? column.db_type);
}
