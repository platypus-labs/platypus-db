// The table editor on a throwaway SQLite database: a column renamed and one added to an
// existing table, with its row kept, then a new table created from the schema's menu.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "Design db";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const editor = `[...document.querySelectorAll('.table-editor')].find(t => t.style.display !== 'none')`;
const modal = `document.querySelector('.modal.sql-action')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "design.db"));
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
           INSERT INTO items VALUES (1, 'pen');`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000021"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "design.db")}'
save_password = false
`,
  );
}

/** Runs the context-menu entry starting with `title` on the navigator row `label`. */
async function menu(page, label, title) {
  await page.eval(`${row(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(title)})).click()`,
  );
}

/** Replaces the text of the editor's input picked by `expression`. */
async function fill(page, expression, text) {
  await page.eval(`(${expression}).id = 'e2e-input'`);
  await page.clickSel("#e2e-input");
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(text);
  await page.eval(`document.getElementById('e2e-input')?.removeAttribute('id')`);
}

/** Saves the editor's changes, checking the script shows `expected`, and waits for the
 *  dialog to close once it ran. */
async function save(page, expected) {
  await page.waitFor(`${editor}.querySelector('.object-header button.primary')?.disabled === false`);
  await page.eval(`${editor}.querySelector('.object-header button.primary').click()`);
  await page.waitFor(`${modal}?.querySelector('pre') !== null`);
  const sql = await page.eval(`${modal}.querySelector('pre').textContent`);
  for (const e of expected) assert.match(sql, e);
  await page.eval(`${modal}.querySelector('button[type=submit]').click()`);
  await page.waitFor(`${modal} === null`, 15_000);
}

export default async function (page, { dir, shots }) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("items")} !== undefined`, 15_000);

  await menu(page, "items", "Edit table");
  await page.waitFor(`${editor}?.querySelectorAll('tbody tr').length === 2`, 10_000);
  const cell = (r, c) => `${editor}.querySelectorAll('tbody tr')[${r}].querySelectorAll('td')[${c}].querySelector('input')`;
  assert.equal(await page.eval(`${cell(1, 0)}.value`), "name");
  await fill(page, cell(1, 0), "title");
  await page.eval(`${editor}.querySelector('.add-row').click()`);
  await page.waitFor(`${editor}.querySelectorAll('tbody tr').length === 3`);
  await fill(page, cell(2, 0), "qty");
  await fill(page, cell(2, 1), "INTEGER");
  if (shots) await page.screenshot(join(shots, "table-editor.png"));
  await save(page, [/RENAME COLUMN name TO title/, /ADD COLUMN qty INTEGER/]);
  // Saved: reloaded from the database, nothing left to save.
  await page.waitFor(`${editor}.querySelector('.object-header button.primary').disabled === true`, 10_000);

  const db = new DatabaseSync(join(dir, "design.db"), { readOnly: true });
  const columns = db.prepare("SELECT name FROM pragma_table_info('items')").all().map((c) => c.name);
  assert.deepEqual(columns, ["id", "title", "qty"]);
  assert.equal(db.prepare("SELECT title FROM items").get().title, "pen");
  db.close();

  // A new table from the schema's menu: one key column to start with, a name to give.
  await menu(page, "main", "Create table");
  await page.waitFor(`${editor}?.querySelector('.table-name input') != null && ${editor}.querySelectorAll('tbody tr').length === 1`, 10_000);
  await fill(page, `${editor}.querySelector('.table-name input')`, "tags");
  await save(page, [/CREATE TABLE main\.tags/, /PRIMARY KEY \(id\)/]);
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('tags')`, 10_000);
  const after = new DatabaseSync(join(dir, "design.db"), { readOnly: true });
  assert.ok(after.prepare("SELECT 1 FROM sqlite_master WHERE name = 'tags'").get(), "tags created");
  after.close();
}
