// GRID-14 (low, UX), all databases. Ctrl+Arrow keys move one cell like the plain arrows, and
// Ctrl+Shift+Down extends the selection by one row: there is no jump to the edge of the data
// (apps/desktop/src/grid/DataGrid.tsx, onKeyDown: the moves ignore Ctrl except for Home/End).
// Selecting a whole column from the keyboard, as in Excel or DBeaver, takes one key press
// per row.
// Steps: focus the first cell of `name` (5 rows), Ctrl+Shift+Down, copy.
// Expected: the five names. Actual: the first two.
import { join } from "node:path";
import { CTRL, SHIFT, assert, connections, copied, editBar, expand, focusCell, openData, stubClipboard } from "../grid-helpers.mjs";
import { DatabaseSync } from "node:sqlite";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "keys.db"));
  db.exec(`CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, city TEXT);
           INSERT INTO t VALUES (1, 'a', 'x'), (2, 'b', NULL), (3, 'c', NULL), (4, 'd', NULL), (5, 'e', NULL);`);
  db.close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "keys.db") }]]);
}

export default async function (page) {
  await expand(page, "QA LITE", null, "t");
  await openData(page, "t");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await stubClipboard(page);
  await focusCell(page, 0, 1);
  await page.key("ArrowDown", "ArrowDown", 40, CTRL | SHIFT);
  assert.equal(await copied(page, () => page.key("c", "KeyC", 67, CTRL)), "a\nb\nc\nd\ne");
  // Ctrl+Right goes to the last column.
  await focusCell(page, 0, 0);
  await page.key("ArrowRight", "ArrowRight", 39, CTRL);
  assert.equal(await copied(page, () => page.key("c", "KeyC", 67, CTRL)), "x");
}
