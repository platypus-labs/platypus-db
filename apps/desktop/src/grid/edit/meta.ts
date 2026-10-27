/** What the data editor needs to know about the table behind a result. */
import type { ColumnInfo, ColumnMeta, Generated, GeneratedColumn, ObjectKind, SourceItem, TableDetails, VirtualKey } from "../../api";
import { msg } from "../../i18n";

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
  /** The key tells every row of the table apart: a primary key or a unique index on all
   *  the rows, not a virtual key nor a primary key that is not unique (ClickHouse). */
  uniqueKey: boolean;
  /** Catalog details by result column; `undefined` for expressions, which stay read-only. */
  columns: (ColumnMeta | undefined)[];
  /** How the database fills each result column by itself, if it does. */
  generated: (Generated | undefined)[];
}

/** Why a result cannot be edited; `noKey` when a virtual key would make it editable. */
export interface ReadOnly {
  reason: string;
  /** Not worth saying, e.g. a query in the SQL editor that joins tables. */
  hidden?: boolean;
  noKey?: { table: TableRef; columns: string[] };
  /** The table and its column behind each result column, when they are known. */
  catalog?: Pick<EditMeta, "table" | "columns">;
}

/** How a column is edited. */
export type EditorKind = "text" | "multiline" | "bool" | "enum" | "date" | "lookup" | "array" | "composite";

/** Finds a catalog column by name: exactly, else ignoring case (MySQL and SQLite). */
function byName(details: TableDetails): (name: string) => ColumnMeta | undefined {
  const exact = new Map(details.columns.map((c) => [c.name, c]));
  const folded = new Map(details.columns.map((c) => [c.name.toLowerCase(), c]));
  return (name) => exact.get(name) ?? folded.get(name.toLowerCase());
}

/** An index column as the catalog names it: PostgreSQL writes the names that need quoting
 *  quoted (`"Order ID"`), as in its index definitions. */
export function unquoted(name: string): string {
  return /^"(?:[^"]|"")*"$/.test(name) ? name.slice(1, -1).replace(/""/g, '"') : name;
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
  if (expected.length !== columns.length) return msg("its columns do not match the query");
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
export function editMeta(
  source: EditSource,
  columns: ColumnInfo[],
  virtualKeys: VirtualKey[],
  generatedColumns: readonly GeneratedColumn[] = [],
): EditMeta | ReadOnly {
  const { table, details } = source;
  const mapped = resultColumns(source, columns);
  if (typeof mapped === "string") return { reason: table.kind === "table" ? mapped : msg("only tables can be edited") };
  // Read-only results still describe their columns, e.g. in the metadata panel.
  const catalog = { table, columns: mapped };
  if (table.kind !== "table") return { reason: msg("only tables can be edited"), catalog };
  if (mapped.every((c) => !c)) return { reason: msg("the result has none of the table's columns") };
  const index = new Map(mapped.flatMap((c, i) => (c ? [[c.name, i] as const] : [])));
  const present = (names: string[]) => names.length > 0 && names.every((n) => index.has(n));
  const find = byName(details);
  const indexes = details.indexes.map((i) => ({ ...i, columns: i.columns.map(unquoted) }));
  // SQLite reports `INTEGER PRIMARY KEY` columns as nullable, though they never hold NULL.
  const primaryIndex = indexes.find((i) => i.primary);
  const primary = primaryIndex?.columns ?? details.columns.filter((c) => c.primary_key).map((c) => c.name);
  // A partial index is unique among the rows of its condition only: it tells no row apart.
  const uniques = indexes.filter(
    (i) => i.unique && !i.partial && i.columns.every((n) => find(n)?.nullable === false),
  );
  const unique = uniques.find((i) => present(i.columns));
  const virtual = virtualKeys
    .find((k) => k.schema === table.schema && k.table === table.name)
    ?.columns.map((n) => find(n)?.name ?? n);
  const kinds = new Map(generatedColumns.map((g) => [g.name, g.kind]));
  const meta = (key: string[], virtualKey: string[] | null, uniqueKey: boolean): EditMeta => ({
    table,
    details,
    key: key.map((n) => index.get(n)!),
    virtualKey,
    uniqueKey,
    columns: mapped,
    generated: mapped.map((c) => (c ? kinds.get(c.name) : undefined)),
  });
  if (present(primary)) return meta(primary, null, !primaryIndex || (primaryIndex.unique && !primaryIndex.partial));
  if (unique) return meta(unique.columns, null, true);
  if (virtual && present(virtual)) return meta(virtual, virtual, false);
  const keyed = primary.length > 0 || uniques.length > 0 || virtual !== undefined;
  if (keyed) return { reason: msg("the result does not include the table's key columns"), catalog };
  return {
    reason: msg("the table has no primary key or unique key on non-null columns"),
    noKey: { table, columns: details.columns.map((c) => c.name) },
    catalog,
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
  // PostgreSQL prints array types as `integer[]`, `character varying(20)[]`. Arrays of
  // boxes separate their elements with `;`, which the list editor splits on.
  if (type.endsWith("[]")) return "array";
  if (meta?.fields.length) return "composite";
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
