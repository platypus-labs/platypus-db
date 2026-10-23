/** What the data editor needs to know about the table behind a result. */
import type { ColumnInfo, ColumnMeta, ObjectKind, TableDetails } from "../../api";

/** The table a data viewer browses. */
export interface TableRef {
  schema: string;
  name: string;
  kind: ObjectKind;
}

export interface EditMeta {
  /** Result columns that identify a row. */
  key: number[];
  /** Catalog details by result column. */
  columns: (ColumnMeta | undefined)[];
}

/** How a column is edited. */
export type EditorKind = "text" | "multiline" | "bool" | "enum" | "date";

/**
 * The key to edit `columns` by: the primary key, else a unique index on non-null columns.
 * Returns why the result is read-only when there is none.
 */
export function editMeta(table: TableRef, columns: ColumnInfo[], details: TableDetails): EditMeta | string {
  if (table.kind !== "table") return "only tables can be edited";
  const byName = new Map(details.columns.map((c) => [c.name, c]));
  const index = new Map(columns.map((c, i) => [c.name, i]));
  const present = (names: string[]) => names.length > 0 && names.every((n) => index.has(n));
  // SQLite reports `INTEGER PRIMARY KEY` columns as nullable, though they never hold NULL.
  const primary =
    details.indexes.find((i) => i.primary)?.columns ??
    details.columns.filter((c) => c.primary_key).map((c) => c.name);
  const unique = details.indexes.find(
    (i) => i.unique && present(i.columns) && i.columns.every((n) => byName.get(n)?.nullable === false),
  );
  const key = present(primary) ? primary : unique?.columns;
  if (!key) return "the table has no primary key or unique key on non-null columns";
  return { key: key.map((n) => index.get(n)!), columns: columns.map((c) => byName.get(c.name)) };
}

const JSON_TYPE = /json/i;
const BOOL_TYPE = /^bool(ean)?$/i;
const DATE = /^\d{4}-\d{2}-\d{2}$/;
/** Longer values get the multi-line editor. */
const LONG_TEXT = 120;

export function editorKind(column: ColumnInfo, meta: ColumnMeta | undefined, value: string | null): EditorKind {
  const type = meta?.data_type ?? column.db_type;
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
