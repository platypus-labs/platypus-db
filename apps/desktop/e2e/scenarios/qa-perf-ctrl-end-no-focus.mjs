// BUG (low, keyboard): with the grid focused but no cell selected yet (focus by Tab or by a click
// on the empty area), Ctrl+End selects the first cell instead of the last one (and Ctrl+Home is
// fine only by chance). onKeyDown in src/grid/DataGrid.tsx falls back to `{ row: 0, col: first }`
// when a move returns null for a missing focus; Ctrl+End/Ctrl+Home need no focused cell.
// On a 1M-row result this is the quickest way to the end, and it silently does nothing.
//   Steps: SELECT 3,000 rows, focus the grid without clicking a cell, Ctrl+End, Ctrl+C.
//   Expected: the last row's last cell ("item 3000").  Actual: the first cell ("1").
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { clipboardAfter, fetchAll, grid, openEditor, run, saveConnections, sqliteEntry } from "../perf.mjs";

const NAME = "QA perf ctrl end";

export function setup({ dir }) {
  const file = join(dir, "end.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
    WITH RECURSIVE s(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM s WHERE g < 3000)
    INSERT INTO items SELECT g, 'item ' || g FROM s;`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

export default async function (page) {
  await openEditor(page, NAME);
  await run(page, "SELECT id, name FROM items ORDER BY id");
  await fetchAll(page);
  await page.eval(`${grid}.focus()`);
  await page.key("End", "End", 35, CTRL);
  // The last block is fetched once scrolled to.
  await page.waitFor(`${grid}.scrollTop > 0`, 5000).catch(() => {});
  await page.sleep(1000);
  const copied = await clipboardAfter(page, "c", "KeyC", 67, CTRL);
  assert.equal(copied, "item 3000");
}
