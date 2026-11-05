/**
 * Rows an agent proposed (`propose_edits`) turned into the data editor's pending changes.
 * A pure module: the hook in TableEditor.tsx loads the key cells, calls `applyProposal`, and
 * records the new state through its own `apply`, so the proposal behaves like the user's own
 * edits (undo, row colors, preview, save, discard, the warning on leaving).
 *
 * The proposal comes from a file any program can write. It is only data here: a row is found
 * by comparing text, a value is checked by the column's rule, and nothing is ever run.
 */
import type { CellInput } from "../../api";
import { checkValue, type ColumnRule, normalize, type Problem } from "./massEdit.ts";
import { addRows, type CellChange, type Cells, deleteRows, type EditState, setCells } from "./state.ts";

/** A row's cells: column name and value as text, `null` for NULL. */
export type ProposedRow = [string, string | null][];

/** The rows proposed for one table, as the app's backend checked and returned them. */
export interface ProposedEdits {
  schema: string;
  table: string;
  inserts: ProposedRow[];
  updates: { key: ProposedRow; set: ProposedRow }[];
  deletes: ProposedRow[];
}

/** A result column as the editor knows it. */
export interface ProposalColumn {
  name: string;
  /** What the column takes; `undefined` when the editor never writes it. */
  rule: ColumnRule | undefined;
  /** Why it is never written: filled in by the database, or not one of the table's columns. */
  fixed?: "database" | "not_table";
}

export type SkipReason =
  /** No row of the result has the key. */
  | "no_row"
  /** Several rows have the key. */
  | "many_rows"
  /** The result does not have the column. */
  | "missing_column"
  /** The column is filled in by the database. */
  | "database"
  /** The column is not one of the table's. */
  | "not_table"
  /** The column does not take the value. */
  | "value";

/** One proposed row left out, and why. */
export interface Skip {
  kind: "insert" | "update" | "delete";
  /** The row's place in its list of the proposal, from 0. */
  index: number;
  reason: SkipReason;
  /** The column concerned; for `value` also the check that failed, in `problem`. */
  column?: string;
  problem?: Problem;
}

export interface Proposal {
  state: EditState;
  /** Proposed rows now pending (rows that already hold the proposed values are not). */
  applied: number;
  skipped: Skip[];
}

const INTEGER = /^[+-]?\d+$/;
const DECIMAL = /^[+-]?\d+\.\d*$/;

/** A cell's text as compared with the proposal's: NULL apart from any text, numbers without
 *  the zeros and signs that do not change them. */
function comparable(rule: ColumnRule | undefined, value: string | null): string {
  if (value === null) return "\u0000null";
  const text = value.trim();
  if (rule?.kind === "integer" && INTEGER.test(text)) {
    const digits = text.replace(/^[+-]/, "").replace(/^0+(?=\d)/, "");
    return text.startsWith("-") && digits !== "0" ? `-${digits}` : digits;
  }
  if (rule?.kind === "number" && (INTEGER.test(text) || DECIMAL.test(text))) {
    const [whole = "", fraction = ""] = text.replace(/^[+-]/, "").split(".");
    const kept = fraction.replace(/0+$/, "");
    const digits = `${whole.replace(/^0+(?=\d)/, "") || "0"}${kept ? `.${kept}` : ""}`;
    return text.startsWith("-") && digits !== "0" ? `-${digits}` : digits;
  }
  return value;
}

/** Whether setting `value` leaves the cell as loaded: the same text, or the same number
 *  written another way (`2.5`, `2.50`). */
function sameValue(rule: ColumnRule | undefined, value: CellInput, was: string | null | undefined): boolean {
  if (was === undefined || value.kind === "default") return false;
  const wanted = value.kind === "null" ? null : value.text;
  return comparable(rule, wanted) === comparable(rule, was);
}

/** The result column called `name`: exactly, else ignoring case when only one fits. */
export function columnIndex(columns: readonly { name: string }[], name: string): number {
  const exact = columns.findIndex((c) => c.name === name);
  if (exact >= 0) return exact;
  const folded = columns.flatMap((c, i) => (c.name.toLowerCase() === name.toLowerCase() ? [i] : []));
  return folded.length === 1 ? folded[0]! : -1;
}

/** The result columns whose loaded values `applyProposal` reads: those of every key, and
 *  of every cell an update sets (to drop a change that equals the loaded value). */
export function neededColumns(proposal: ProposedEdits, columns: readonly { name: string }[]): number[] {
  const wanted = new Set<number>();
  const take = (row: ProposedRow) => {
    for (const [name] of row) {
      const col = columnIndex(columns, name);
      if (col >= 0) wanted.add(col);
    }
  };
  for (const u of proposal.updates) {
    take(u.key);
    take(u.set);
  }
  for (const key of proposal.deletes) take(key);
  return [...wanted].sort((a, b) => a - b);
}

const input = (value: string | null): CellInput => (value === null ? { kind: "null" } : { kind: "value", text: value });

/** The columns and checked values of `row`, or the first reason it cannot be written. */
function cellsOf(row: ProposedRow, columns: readonly ProposalColumn[]): { cells: [number, CellInput][] } | Omit<Skip, "kind" | "index"> {
  const cells: [number, CellInput][] = [];
  for (const [name, value] of row) {
    const col = columnIndex(columns, name);
    const column = columns[col];
    if (!column) return { reason: "missing_column", column: name };
    if (!column.rule) return { reason: column.fixed ?? "not_table", column: name };
    const wanted = normalize(column.rule, input(value));
    const problem = checkValue(column.rule, wanted);
    if (problem) return { reason: "value", column: name, problem };
    cells.push([col, wanted]);
  }
  return { cells };
}

/**
 * The pending changes of `proposal` on top of `state`. `loaded(row, col)` is the full text of
 * a result row's cell for the columns of `neededColumns`; `rowCount` the rows of the result.
 *
 * A row is found by its key: the cells of the key's columns as loaded must equal the proposal's
 * text, column by column and by column name. A proposed row that cannot be applied (no row has
 * the key, a column is missing or never written, a value the column refuses) is left out
 * whole and listed in `skipped`: nothing is dropped silently, nothing is half applied.
 */
export function applyProposal(
  state: EditState,
  proposal: ProposedEdits,
  columns: readonly ProposalColumn[],
  rowCount: number,
  loaded: (row: number, col: number) => string | null | undefined,
): Proposal {
  const skipped: Skip[] = [];
  // Result rows sit under the new rows in the grid: applied against the rows as they are now,
  // before the proposal's own new rows go on top.
  const offset = state.inserts.length;
  const indexes = new Map<string, Map<string, number[]>>();
  const find = (key: ProposedRow): { row: number } | { skip: Omit<Skip, "kind" | "index"> } => {
    const cols: number[] = [];
    for (const [name] of key) {
      const col = columnIndex(columns, name);
      if (col < 0) return { skip: { reason: "missing_column", column: name } };
      cols.push(col);
    }
    const signature = cols.join(",");
    let byKey = indexes.get(signature);
    if (!byKey) {
      byKey = new Map();
      for (let row = 0; row < rowCount; row++) {
        const id = JSON.stringify(cols.map((c) => comparable(columns[c]?.rule, loaded(row, c) ?? null)));
        byKey.set(id, [...(byKey.get(id) ?? []), row]);
      }
      indexes.set(signature, byKey);
    }
    const wanted = JSON.stringify(key.map(([name, value]) => comparable(columns[columnIndex(columns, name)]?.rule, value)));
    const rows = byKey.get(wanted) ?? [];
    if (rows.length === 0) return { skip: { reason: "no_row" } };
    if (rows.length > 1) return { skip: { reason: "many_rows" } };
    return { row: rows[0]! };
  };

  let next = state;
  let applied = 0;
  const changes: CellChange[] = [];
  const touched = new Set<number>();
  proposal.updates.forEach((update, index) => {
    const found = find(update.key);
    if ("skip" in found) return void skipped.push({ kind: "update", index, ...found.skip });
    const cells = cellsOf(update.set, columns);
    if (!("cells" in cells)) return void skipped.push({ kind: "update", index, ...cells });
    for (const [col, value] of cells.cells) {
      const was = loaded(found.row, col);
      if (!sameValue(columns[col]?.rule, value, was)) changes.push({ gridRow: found.row + offset, col, value, original: was });
    }
    touched.add(found.row);
  });
  next = setCells(next, changes);
  applied += [...touched].filter((row) => next.updates.has(row)).length;

  const gone: number[] = [];
  proposal.deletes.forEach((key, index) => {
    const found = find(key);
    if ("skip" in found) return void skipped.push({ kind: "delete", index, ...found.skip });
    if (!next.deletes.has(found.row)) gone.push(found.row + offset);
  });
  next = deleteRows(next, gone);
  applied += gone.length;

  const added: Cells[] = [];
  proposal.inserts.forEach((row, index) => {
    const cells = cellsOf(row, columns);
    if (!("cells" in cells)) return void skipped.push({ kind: "insert", index, ...cells });
    added.push(new Map(cells.cells));
  });
  next = addRows(next, added);
  applied += added.length;
  return { state: next, applied, skipped };
}
