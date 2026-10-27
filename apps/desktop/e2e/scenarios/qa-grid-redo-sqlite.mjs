// GRID-13 (low, UX), all databases. The data editor has undo (Ctrl+Z, a stack of states) but no
// redo: Ctrl+Y and Ctrl+Shift+Z do nothing, so a change undone by mistake must be typed again
// (apps/desktop/src/grid/edit/TableEditor.tsx keeps only `undoStack`; grid/edit/actions.ts
// registers grid.undo_change only). DBeaver's grid redoes with Ctrl+Y.
// Steps: edit two cells, Ctrl+Z twice, Ctrl+Y.
// Expected: the first edit comes back. Actual: nothing is pending.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, assert, connections, editBar, expand, focusCell, grid, openData, previewSql, setCell } from "../qa-grid.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "redo.db"));
  db.exec(`CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT);
           INSERT INTO t VALUES (1, 'a'), (2, 'b');`);
  db.close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "redo.db") }]]);
}

export default async function (page) {
  await expand(page, "QA LITE", null, "t");
  await openData(page, "t");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await focusCell(page, 0, 1);
  await setCell(page, "x");
  await focusCell(page, 1, 1);
  await setCell(page, "y");
  await page.waitFor(`${editBar}.includes('2 rows changed')`);
  await page.eval(`${grid}.focus()`);
  await page.key("z", "KeyZ", 90, CTRL);
  await page.key("z", "KeyZ", 90, CTRL);
  await page.waitFor(`${editBar}.includes('to edit a cell')`);
  await page.key("y", "KeyY", 89, CTRL);
  await page.waitFor(`${editBar}.includes('1 row changed')`, 3_000);
  assert.equal(await previewSql(page), "UPDATE main.t SET name = 'x' WHERE id = 1;");
}
