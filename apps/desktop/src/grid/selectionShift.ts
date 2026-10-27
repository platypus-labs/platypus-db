/**
 * The grid's selection when the rows drawn above the result (the data editor's new rows) go
 * from `before` to `after`: it stays on the same rows. Result rows move by the difference;
 * new rows come and go on top, so they move by it too. A selection end on a new row that is
 * gone clears the selection rather than land on another row, where Delete would mark it.
 */

export interface RowCell {
  row: number;
  col: number;
}

export function shiftedRow(row: number, before: number, after: number): number | null {
  const moved = row + after - before;
  return row < before && moved < 0 ? null : moved;
}

/** Anchor and focus moved; `null` when the selection has to go. */
export function shiftSelection<C extends RowCell>(
  anchor: C,
  focus: C,
  before: number,
  after: number,
): { anchor: C; focus: C } | null {
  const a = shiftedRow(anchor.row, before, after);
  const f = shiftedRow(focus.row, before, after);
  if (a === null || f === null) return null;
  return { anchor: { ...anchor, row: a }, focus: { ...focus, row: f } };
}
