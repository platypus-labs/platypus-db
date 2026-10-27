// Unit tests of the grid selection following its rows when new rows come and go above the
// result (src/grid/selectionShift.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { shiftSelection, shiftedRow } from "../src/grid/selectionShift.ts";

const cell = (row, col = 0) => ({ row, col });

test("undoing an added row keeps the selection on the same result rows", () => {
  // One new row on top: result rows 10-12 are grid rows 11-13.
  assert.deepEqual(shiftSelection(cell(11), cell(13), 1, 0), { anchor: cell(10), focus: cell(12) });
});

test("duplicating rows keeps the selection on the rows that were duplicated", () => {
  // Rows 3-4 duplicated: two new rows on top.
  assert.deepEqual(shiftSelection(cell(3, 1), cell(4, 2), 0, 2), { anchor: cell(5, 1), focus: cell(6, 2) });
});

test("discarding every new row moves result rows up by their number", () => {
  assert.equal(shiftedRow(5, 3, 0), 2);
  assert.equal(shiftedRow(3, 3, 0), 0);
});

test("a new row still there moves with the others", () => {
  assert.equal(shiftedRow(1, 2, 1), 0);
  assert.equal(shiftedRow(0, 1, 3), 2);
});

test("a selection on a new row that is gone is cleared, not moved onto a result row", () => {
  assert.equal(shiftedRow(0, 1, 0), null);
  assert.equal(shiftSelection(cell(0), cell(4), 1, 0), null);
});

test("no change, no move", () => {
  assert.deepEqual(shiftSelection(cell(2), cell(2), 1, 1), { anchor: cell(2), focus: cell(2) });
});
