// BUG (high, wrong data): Ctrl+C on a cell with a long text copies the grid's display text, cut to
// 500 characters with an ellipsis, not the value. Same root cause as perf-copy-unloaded-rows:
// grid.copy (src/grid/actions.ts) copies the client cache, which holds the values cut by the
// backend for display (MAX_CELL_CHARS = 500 in src-tauri/src/display.rs). "Copy with headers"
// copies the whole value.
//   Steps: SELECT a row whose text is 10,000 characters, click the cell, Ctrl+C.
//   Expected: the clipboard holds the 10,000 characters.
//   Actual: 501 characters (500 and "…"); with a 6 MB PostgreSQL text the same.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { clipboardAfter, gridAt, openEditor, run, saveConnections, sqliteEntry, visibleTab } from "../perf.mjs";

const NAME = "QA perf long cell";
const LENGTH = 10_000;

export function setup({ dir }) {
  const file = join(dir, "long.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE docs (id INTEGER PRIMARY KEY, body TEXT);
    INSERT INTO docs VALUES (1, substr(replace(hex(zeroblob(${LENGTH})), '00', 'ab'), 1, ${LENGTH}));`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

export default async function (page) {
  await openEditor(page, NAME);
  await run(page, "SELECT body FROM docs");
  // Until its rows arrive the column is as wide as its header, and the click would land to
  // its right: wait until the grid has sized it to the text.
  await page.waitFor(`${visibleTab}.querySelector('.grid-canvas')?.dataset.measures !== undefined`);
  const g = await gridAt(page);
  await page.click(g.x + 120, g.y + 60);
  await page.waitFor(`document.activeElement?.classList.contains('grid-scroll') === true`);
  const text = await clipboardAfter(page, "c", "KeyC", 67, CTRL);
  assert.equal(text.length, LENGTH, `copied ${text.length} characters, ending ${JSON.stringify(text.slice(-5))}`);
  assert.equal(text, "ab".repeat(LENGTH / 2));
}
