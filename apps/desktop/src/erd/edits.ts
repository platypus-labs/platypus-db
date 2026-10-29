/** The edit mode of the ER diagram: the schema is drawn and changed on the diagram, and the
 *  edits become a script. Here the pure part: what is kept with the tab, how a table is
 *  drawn from its database details or from its edited design, and how edits are made. */
import type { ForeignKeyDesign, SchemaTable, TableDesign } from "../api";

/** The edits kept with the tab. */
export interface ErdEdits {
  /** The wanted design of each table created or changed. */
  designs: TableDesign[];
  /** The tables to drop. */
  dropped: string[];
}

export const emptyEdits = (): ErdEdits => ({ designs: [], dropped: [] });

/** How many changes are pending: a table created or changed, a table dropped. */
export const pendingCount = (edits: ErdEdits) => edits.designs.length + edits.dropped.length;

/** What a table of the diagram is, once the edits are applied to it. */
export type TableState = "plain" | "new" | "changed" | "dropped";

/** A table as the diagram draws it, from the database or from an edited design. */
export interface Drawn {
  name: string;
  comment: string | null;
  /** Why the details could not be read. */
  error: string | null;
  state: TableState;
  columns: { name: string; data_type: string; nullable: boolean; primary_key: boolean }[];
  foreign_keys: { name: string; columns: string[]; ref_schema: string; ref_table: string; ref_columns: string[] }[];
}

const drawnFromDetails = (t: SchemaTable, state: TableState): Drawn => ({
  name: t.name,
  comment: t.comment,
  error: t.error,
  state,
  columns: t.details.columns,
  foreign_keys: t.details.foreign_keys,
});

const drawnFromDesign = (d: TableDesign, state: TableState, comment: string | null): Drawn => ({
  name: d.name,
  comment,
  error: null,
  state,
  columns: d.columns.map((c) => ({ name: c.name, data_type: c.data_type, nullable: !c.primary_key && c.nullable, primary_key: c.primary_key })),
  foreign_keys: d.foreign_keys,
});

/** The tables to draw: those read, each drawn from its edited design when it has one (or
 *  marked dropped), then the tables created, which the database does not have yet. */
export function drawnTables(read: SchemaTable[], edits: ErdEdits | null): Drawn[] {
  if (!edits) return read.map((t) => drawnFromDetails(t, "plain"));
  const design = (name: string) => edits.designs.find((d) => d.name === name);
  const known = new Set(read.map((t) => t.name));
  const drawn = read.map((t) => {
    const edited = design(t.name);
    if (edits.dropped.includes(t.name)) return drawnFromDetails(t, "dropped");
    return edited ? drawnFromDesign(edited, "changed", t.comment) : drawnFromDetails(t, "plain");
  });
  for (const d of edits.designs) if (!known.has(d.name)) drawn.push(drawnFromDesign(d, "new", null));
  return drawn;
}

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** Why a design cannot be added to the diagram, or `null` when it can. `taken` are the names
 *  of the tables the schema has already, those created included. */
export type DesignProblem = "name" | "duplicate" | "columns" | "column_name" | "column_type" | "duplicate_column";

export function designProblem(design: TableDesign, taken: string[]): DesignProblem | null {
  if (design.name.trim() === "") return "name";
  if (taken.some((n) => same(n, design.name))) return "duplicate";
  if (design.columns.length === 0) return "columns";
  if (design.columns.some((c) => c.name.trim() === "")) return "column_name";
  if (design.columns.some((c) => c.data_type.trim() === "")) return "column_type";
  const seen = new Set(design.columns.map((c) => c.name.toLowerCase()));
  if (seen.size !== design.columns.length) return "duplicate_column";
  return null;
}

/** `edits` with `design` in the place of the one of the same name, or added. A table dropped
 *  is not dropped any more. */
export function withDesign(edits: ErdEdits, design: TableDesign): ErdEdits {
  const has = edits.designs.some((d) => d.name === design.name);
  return {
    designs: has ? edits.designs.map((d) => (d.name === design.name ? design : d)) : [...edits.designs, design],
    dropped: edits.dropped.filter((n) => n !== design.name),
  };
}

/** The other designs without the foreign keys to `table`, which is gone from the diagram. */
const withoutKeysTo = (designs: TableDesign[], table: string): TableDesign[] =>
  designs.map((d) => ({ ...d, foreign_keys: d.foreign_keys.filter((f) => f.ref_table !== table) }));

/** `edits` without the design of `name`: a table created is gone, a table changed is as the
 *  database has it. The foreign keys that other designs had to it go too. */
export function withoutDesign(edits: ErdEdits, name: string, isNew: boolean): ErdEdits {
  const designs = edits.designs.filter((d) => d.name !== name);
  return { ...edits, designs: isNew ? withoutKeysTo(designs, name) : designs };
}

/** `edits` with the table created `from` saved as `design`, which may have another name:
 *  the foreign keys to it follow. */
export function withRenamedNew(edits: ErdEdits, from: string, design: TableDesign): ErdEdits {
  const keys = (d: TableDesign) => ({ ...d, foreign_keys: d.foreign_keys.map((f) => (f.ref_table === from ? { ...f, ref_table: design.name } : f)) });
  return {
    ...edits,
    designs: edits.designs.map((d) => (d.name === from ? design : keys(d))),
  };
}

/** `edits` with the table `name` dropped, its design removed. */
export function withDropped(edits: ErdEdits, name: string): ErdEdits {
  return {
    designs: withoutKeysTo(
      edits.designs.filter((d) => d.name !== name),
      name,
    ),
    dropped: [...edits.dropped.filter((n) => n !== name), name],
  };
}

/** `edits` with the table `name` kept after all. */
export const withoutDrop = (edits: ErdEdits, name: string): ErdEdits => ({
  ...edits,
  dropped: edits.dropped.filter((n) => n !== name),
});

/** The foreign key a drag from `column` of `design` onto `target` makes, or why it cannot be
 *  made: the target needs a primary key of one column, and the key must be new. */
export function dragKey(design: TableDesign, column: string, target: Drawn, schema: string): { key: ForeignKeyDesign } | { problem: "no_key" | "composite_key" | "exists" } {
  const keys = target.columns.filter((c) => c.primary_key);
  if (keys.length === 0) return { problem: "no_key" };
  if (keys.length > 1) return { problem: "composite_key" };
  const ref = keys[0]!.name;
  const dup = design.foreign_keys.some(
    (f) => f.columns.length === 1 && f.columns[0] === column && f.ref_table === target.name && f.ref_columns[0] === ref && (!f.ref_schema || f.ref_schema === schema),
  );
  if (dup) return { problem: "exists" };
  const base = `fk_${design.name}_${column}`;
  const names = new Set(design.foreign_keys.map((f) => f.name));
  let name = base;
  for (let n = 2; names.has(name); n++) name = `${base}_${n}`;
  return {
    key: { original: null, name, columns: [column], ref_schema: schema, ref_table: target.name, ref_columns: [ref], on_delete: null, on_update: null },
  };
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isDesign = (v: unknown): v is TableDesign =>
  isObject(v) &&
  typeof v.name === "string" &&
  [v.columns, v.indexes, v.foreign_keys, v.constraints].every(Array.isArray) &&
  (v.columns as unknown[]).every((c) => isObject(c) && typeof c.name === "string" && typeof c.data_type === "string");

/** The edits read back from what was saved with the tab; `null` (not in edit mode) when it is
 *  not edits. A design of another shape is dropped. */
export function restoreEdits(saved: unknown): ErdEdits | null {
  if (!isObject(saved)) return null;
  return {
    designs: Array.isArray(saved.designs) ? saved.designs.filter(isDesign) : [],
    dropped: Array.isArray(saved.dropped) ? saved.dropped.filter((n): n is string => typeof n === "string") : [],
  };
}
