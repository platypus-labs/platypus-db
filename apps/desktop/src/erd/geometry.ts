/** The sizes of an ER diagram's tables, shared by the tab that draws it and the SVG an agent asks
 *  for: the pure part, with no React and no DOM. */
import { truncate } from "../analysis/chartScale.ts";
import type { Box } from "./layout";

export const HEAD = 28;
export const ROW = 22;
/** The room of the key mark (PK, FK) at the start of a column row. */
export const MARK = 24;
/** The room of the marks of a column that is both a primary key and a foreign key. */
export const MARK_BOTH = 2 * MARK - 2;
export const CHAR = 7;
export const PAD = 20;

/** What of a table the sizes depend on. */
export interface Shape {
  name: string;
  columns: { name: string; data_type: string; nullable: boolean; primary_key: boolean }[];
  foreign_keys: { name: string; columns: string[]; ref_schema?: string; ref_table: string; ref_columns: string[] }[];
}

/** The characters of a column row that the name has: the box less the key mark, the type and
 *  the margins. */
export const nameChars = (width: number, type: string, mark = MARK) => Math.max(4, Math.floor((width - mark - 6 - 10 - 8 - type.length * 6.5) / CHAR));

export const typeText = (type: string) => truncate(type, 24);

/** The columns of `table` that are part of a foreign key. */
export const fkColumns = (table: Shape) => new Set(table.foreign_keys.flatMap((f) => f.columns));

/** The width of a table's box; `tag` is the room (in characters) of the label edits put in its head. */
const boxWidth = (table: Shape, tag = 0) => {
  const fks = fkColumns(table);
  const both = table.columns.some((c) => c.primary_key && fks.has(c.name));
  const longest = Math.max(table.name.length + 2 + tag, ...table.columns.map((c) => c.name.length + c.data_type.length + 3));
  return Math.min(340, Math.max(140, 16 + (both ? MARK_BOTH : MARK) + CHAR * longest));
};

/** Whether `fk` references a table of `schema`, the diagram's: a table of another schema is
 *  not drawn, even with the name of one here. */
export const local = (fk: { ref_schema?: string }, schema: string) => !fk.ref_schema || fk.ref_schema === schema;

export const boxOf = (table: Shape, schema: string, tag = 0): Box => ({
  name: table.name,
  width: boxWidth(table, tag),
  height: HEAD + ROW * table.columns.length + 6,
  refs: table.foreign_keys
    .filter((f) => local(f, schema))
    .map((f) => f.ref_table)
    .filter((r) => r !== table.name),
});

/** Where the line of a foreign key meets `table`: the middle of the row of `column` (from the
 *  top of the box), or of its primary key when the column is not there. */
export function rowY(table: Shape, column?: string): number {
  let i = table.columns.findIndex((c) => c.name === column);
  if (i < 0) i = Math.max(0, table.columns.findIndex((c) => c.primary_key));
  return HEAD + i * ROW + ROW / 2;
}
