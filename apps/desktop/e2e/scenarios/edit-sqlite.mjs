// The data editor on a throwaway SQLite database: open a table from the navigator, change a
// cell by typing, delete a row, add one, preview the SQL and save; a table without a key
// stays read-only and says why.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E edit";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const path = join(dir, "edit.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
           INSERT INTO items VALUES (1, 'a'), (2, 'b'), (3, 'c');
           CREATE TABLE nokey (v TEXT);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000002"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

async function openTable(page, name) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`${treeRow(name)}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${visibleTab}?.querySelector('.edit-bar') != null`, 15_000);
}

/** Types into the cell editor once it is open, then presses Enter. */
async function finishEdit(page, text) {
  await page.waitFor(`document.querySelector('.cell-editor') !== null`);
  // Same font as the grid it sits on, so the text does not jump when editing starts.
  assert.match(await page.eval(`getComputedStyle(document.querySelector('.cell-editor')).fontFamily`), /^"?JetBrains Mono/);
  if (text) await page.type(text);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  // The current schema opens by itself.
  await page.eval(`${treeRow(NAME)}.click()`);

  await openTable(page, "items");
  await page.waitFor(`${status} === '3 rows'`);
  await page.waitFor(`${editBar}.includes('to edit a cell')`);

  const grid = `${visibleTab}.querySelector('.grid-scroll')`;
  await page.eval(`${grid}.focus()`);
  // The first arrow focuses the top-left cell; then move to `name`.
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("ArrowRight", "ArrowRight", 39);
  // Typing starts editing with that key; Enter saves the cell and moves down.
  await page.key("z", "KeyZ", 90);
  await finishEdit(page, "ed");
  await page.key("Delete", "Delete", 46);
  await page.key("Insert", "Insert", 45);
  await page.key("F2", "F2", 113);
  await finishEdit(page, "x");
  assert.match(await page.eval(editBar), /3 rows changed/);

  await page.eval(`[...${visibleTab}.querySelectorAll('.edit-bar button')].find(b => b.textContent === 'Preview SQL').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') !== null`);
  const sql = await page.eval(`document.querySelector('.sql-preview').textContent`);
  assert.equal(
    sql,
    "DELETE FROM main.items WHERE id = 2;\n" +
      "UPDATE main.items SET name = 'zed' WHERE id = 1;\n" +
      "INSERT INTO main.items (name) VALUES ('x');",
  );
  // Blank the status, so the wait below sees the reload's and not the one before it.
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
  // Saving reloads the table.
  await page.waitFor(`${editBar}.includes('to edit a cell') && ${status} === '3 rows'`, 15_000);

  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
  await page.eval(`${grid}.focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.key("c", "KeyC", 67, CTRL);
  await page.waitFor(`window.__copied !== undefined`);
  assert.equal(await page.eval(`window.__copied`), "1\tzed\n3\tc\n4\tx");

  await openTable(page, "nokey");
  await page.waitFor(`${editBar}.startsWith('Read-only')`);
  assert.match(await page.eval(editBar), /no primary key/);
}
