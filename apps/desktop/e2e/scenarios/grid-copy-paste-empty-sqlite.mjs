// GRID-15 (low), all databases. Copying cells and pasting them back in the grid turns empty
// strings into NULL: the copy writes NULL and '' both as nothing (apps/desktop/src-tauri/src/
// copy.rs, delimited_rows: `unwrap_or("")`), and the paste reads every empty field as NULL
// (apps/desktop/src/grid/edit/paste.ts callers in TableEditor.tsx, paste: "empty fields are
// NULL"). Copying a row over another (Ctrl+C, Ctrl+V) to make them equal changes '' into NULL,
// which a NOT NULL column then refuses.
// Steps: row 1 has name '' and note NULL; copy name..note of row 1, paste over row 2.
// Expected: row 2 gets name '' and note NULL. Actual: name = NULL.
import { join } from "node:path";
import { spawnSync } from "../proc.mjs";
import { DatabaseSync } from "node:sqlite";
import { CTRL, SHIFT, assert, connections, copied, editBar, expand, focusCell, openData, previewSql, stubClipboard } from "../grid-helpers.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "empty.db"));
  db.exec(`CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL, note TEXT);
           INSERT INTO t VALUES (1, '', NULL), (2, 'b', 'c');`);
  db.close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "empty.db") }]]);
}

export default async function (page) {
  await expand(page, "QA LITE", null, "t");
  await openData(page, "t");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await stubClipboard(page);
  await focusCell(page, 0, 1);
  await page.key("ArrowRight", "ArrowRight", 39, SHIFT);
  const text = await copied(page, () => page.key("c", "KeyC", 67, CTRL));
  // The stub kept the text from the system clipboard: put it there for the paste.
  const r = spawnSync("powershell", ["-NoProfile", "-Command", "Set-Clipboard -Value $env:E2E_CLIP"], { env: { ...process.env, E2E_CLIP: text } });
  assert.equal(r.status, 0);
  await focusCell(page, 1, 1);
  await page.key("v", "KeyV", 86, CTRL);
  await page.waitFor(`${editBar}.includes('1 row changed')`);
  assert.equal(await previewSql(page), "UPDATE main.t SET name = '', note = NULL WHERE id = 2;");
}
