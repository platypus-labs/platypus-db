/**
 * Pending changes of the data editor. The state is immutable: every change returns a new
 * one, so undo is a stack of previous states and React sees each change.
 *
 * Grid rows count the new rows first (shown above the result), then the result's rows.
 */
import type { CellInput, ColumnValue, TableChanges } from "../../api";

export type Cells = ReadonlyMap<number, CellInput>;

export interface EditState {
  /** Changed cells of result rows: result row → column → value. */
  readonly updates: ReadonlyMap<number, Cells>;
  /** New rows, in grid order (added ones go on top): column → value; columns left out
   *  take their default. */
  readonly inserts: readonly Cells[];
  /** Result rows to delete. */
  readonly deletes: ReadonlySet<number>;
}

export const EMPTY: EditState = { updates: new Map(), inserts: [], deletes: new Set() };

/** A grid row: one of the new rows, or a row of the result. */
export type RowRef = { insert: number } | { row: number };

export function rowRef(state: EditState, gridRow: number): RowRef {
  const n = state.inserts.length;
  return gridRow < n ? { insert: gridRow } : { row: gridRow - n };
}

export function isEmpty(state: EditState): boolean {
  return state.updates.size === 0 && state.inserts.length === 0 && state.deletes.size === 0;
}

/** Changed, new and deleted rows. */
export function changedRows(state: EditState): number {
  return state.updates.size + state.inserts.length + state.deletes.size;
}

/** The pending value of a cell, if it was changed. */
export function cellEdit(state: EditState, gridRow: number, col: number): CellInput | undefined {
  const ref = rowRef(state, gridRow);
  return "insert" in ref ? state.inserts[ref.insert]?.get(col) : state.updates.get(ref.row)?.get(col);
}

function same(a: CellInput, original: string | null | undefined): boolean {
  if (original === undefined) return false;
  return a.kind === "null" ? original === null : a.kind === "value" && a.text === original;
}

export interface CellChange {
  gridRow: number;
  col: number;
  value: CellInput;
  /** The value as loaded, if known: setting it back drops the change. */
  original?: string | null;
}

export function setCells(state: EditState, changes: CellChange[]): EditState {
  let inserts: Cells[] | null = null;
  let updates: Map<number, Cells> | null = null;
  for (const { gridRow, col, value, original } of changes) {
    const ref = rowRef(state, gridRow);
    if ("insert" in ref) {
      inserts ??= [...state.inserts];
      const row = new Map(inserts[ref.insert]);
      row.set(col, value);
      inserts[ref.insert] = row;
      continue;
    }
    if (state.deletes.has(ref.row)) continue;
    updates ??= new Map(state.updates);
    const row = new Map(updates.get(ref.row));
    if (same(value, original)) row.delete(col);
    else row.set(col, value);
    if (row.size) updates.set(ref.row, row);
    else updates.delete(ref.row);
  }
  return { ...state, inserts: inserts ?? state.inserts, updates: updates ?? state.updates };
}

/**
 * `changes` worked out against `base`, moved to the grid rows their rows are at in `state`:
 * rows added or dropped above them meanwhile shift them. A result row is found by its index;
 * a new row by its cells, so changes of a new row that was dropped or edited meanwhile are
 * left out, as are those of result rows deleted meanwhile.
 */
export function rebase(base: EditState, state: EditState, changes: readonly CellChange[]): CellChange[] {
  if (base === state) return [...changes];
  const n = state.inserts.length;
  const newRows = new Map(state.inserts.map((cells, i) => [cells, i]));
  const out: CellChange[] = [];
  for (const change of changes) {
    const ref = rowRef(base, change.gridRow);
    let gridRow: number | undefined;
    if ("row" in ref) gridRow = state.deletes.has(ref.row) ? undefined : ref.row + n;
    else {
      const cells = base.inserts[ref.insert];
      gridRow = cells && newRows.get(cells);
    }
    if (gridRow !== undefined) out.push(gridRow === change.gridRow ? change : { ...change, gridRow });
  }
  return out;
}

/** Adds new rows on top, in the order given. */
export function addRows(state: EditState, rows: Cells[]): EditState {
  return { ...state, inserts: [...rows, ...state.inserts] };
}

/** New rows are dropped; result rows are marked for deletion and lose their changes. */
export function deleteRows(state: EditState, gridRows: Iterable<number>): EditState {
  const dropped = new Set<number>();
  const deletes = new Set(state.deletes);
  const updates = new Map(state.updates);
  for (const gridRow of gridRows) {
    const ref = rowRef(state, gridRow);
    if ("insert" in ref) {
      dropped.add(ref.insert);
    } else {
      deletes.add(ref.row);
      updates.delete(ref.row);
    }
  }
  const inserts = dropped.size ? state.inserts.filter((_, i) => !dropped.has(i)) : state.inserts;
  return { updates, inserts, deletes };
}

const values = (cells: Cells): ColumnValue[] => [...cells].map(([column, value]) => ({ column, value }));

/** The request for `api.saveChanges`; new rows are inserted in grid order, so pasted rows
 *  keep theirs. */
export function toChanges(
  state: EditState,
  target: Pick<TableChanges, "table" | "key" | "unique_key" | "table_columns" | "defaults">,
): TableChanges {
  return {
    ...target,
    updates: [...state.updates].map(([row, cells]) => ({ row, values: values(cells) })),
    inserts: state.inserts.map(values),
    deletes: [...state.deletes],
  };
}
