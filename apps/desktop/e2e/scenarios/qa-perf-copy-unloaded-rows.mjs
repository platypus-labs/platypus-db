// BUG (critical, wrong data): Ctrl+C copies an empty line for every selected row the grid has not
// drawn yet. The plain copy (grid.copy in src/grid/actions.ts) reads the grid's client cache
// (RowCache: blocks of 500 rows, fetched only when scrolled into view, at most 100 blocks kept),
// not the backend result, while "Copy with headers"/"Copy as" ask the backend (copy_rows).
//   Steps: SELECT a 2,000-row table, Ctrl+A, Ctrl+C without scrolling.
//   Expected: 2,000 lines with the rows' values.
//   Actual: rows 1-500 are copied, rows 501-2,000 are empty lines (on a fetched 1M-row result,
//   990,500 of the 1,000,000 lines are empty). Pasted elsewhere, the data is silently lost.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { clipboardAfter, fetchAll, grid, openEditor, run, saveConnections, sqliteEntry } from "../perf.mjs";

const NAME = "QA perf copy";
const ROWS = 2000;

export function setup({ dir }) {
  const file = join(dir, "copy.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
    WITH RECURSIVE s(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM s WHERE g < ${ROWS})
    INSERT INTO items SELECT g, 'item ' || g FROM s;`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

export default async function (page) {
  await openEditor(page, NAME);
  await run(page, "SELECT id, name FROM items ORDER BY id");
  // Every row is in the backend's result (the first page is 1,500 rows; fetch the rest).
  await fetchAll(page);
  await page.eval(`${grid}.focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  const text = await clipboardAfter(page, "c", "KeyC", 67, CTRL);
  const lines = text.split("\n");
  assert.equal(lines.length, ROWS);
  const empty = lines.filter((l) => /^\t*$/.test(l)).length;
  assert.equal(lines[0], "1\titem 1");
  assert.equal(empty, 0, `${empty} of ${ROWS} copied lines are empty (rows the grid has not drawn)`);
  assert.equal(lines.at(-1), `${ROWS}\titem ${ROWS}`);
}
