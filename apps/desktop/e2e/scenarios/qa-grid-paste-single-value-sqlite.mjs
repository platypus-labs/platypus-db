// GRID-04 (high), all databases. Pasting one value (Ctrl+V) that contains a comma or a
// semicolon splits it over several cells: the clipboard text is read as CSV whenever its first
// line has no tab (apps/desktop/src/grid/edit/paste.ts, delimiterOf/parseClipboard). Text
// copied from anywhere but a spreadsheet changes the focused cell and the cells to its right:
// "Paris, France" writes name = 'Paris' and note = ' France'; a JSON value {"x": 1, "y": 2}
// becomes '{"x": 1' (invalid JSON) plus ' "y": 2}' in the next column; a value that starts with
// a double quote loses its quotes ('"quoted" word' -> 'quoted word').
// Expected: text without a tab or a line break is one value for the focused cell, as typed.
// Actual: it is split as CSV and the values saved are wrong.
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, assert, button, connections, editBar, expand, focusCell, openData, previewSql, visibleTab } from "../qa-grid.mjs";

function setClipboard(text) {
  const r = spawnSync("powershell", ["-NoProfile", "-Command", "Set-Clipboard -Value $env:E2E_CLIP"], {
    env: { ...process.env, E2E_CLIP: text },
  });
  assert.equal(r.status, 0, String(r.stderr));
}

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "paste.db"));
  db.exec(`CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, note TEXT);
           INSERT INTO t VALUES (1, 'a', 'b');`);
  db.close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "paste.db") }]]);
}

/** Pastes `text` over the name of the first row; returns the statement saving would run. */
async function pasted(page, text) {
  setClipboard(text);
  await focusCell(page, 0, 1);
  await page.key("v", "KeyV", 86, CTRL);
  await page.waitFor(`${editBar}.includes('1 row changed')`);
  const sql = await previewSql(page);
  await page.eval(`${button(visibleTab, "Discard")}.click()`);
  await page.waitFor(`${editBar}.includes('to edit a cell')`);
  return sql;
}

export default async function (page) {
  await expand(page, "QA LITE", null, "t");
  await openData(page, "t");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  assert.equal(await pasted(page, "Paris, France"), "UPDATE main.t SET name = 'Paris, France' WHERE id = 1;");
  assert.equal(await pasted(page, "a; b"), "UPDATE main.t SET name = 'a; b' WHERE id = 1;");
  assert.equal(await pasted(page, '{"x": 1, "y": 2}'), `UPDATE main.t SET name = '{"x": 1, "y": 2}' WHERE id = 1;`);
  assert.equal(await pasted(page, '"quoted" word'), `UPDATE main.t SET name = '"quoted" word' WHERE id = 1;`);
}
