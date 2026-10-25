// Import of data files: a CSV into an existing SQLite table, then into a new table created
// from the inferred types, both confirmed as on any production connection; and a CSV opened
// as a database, as when dropped on the window.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const NAME = "Import db";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const dialog = `document.querySelector('.modal.import-data')`;
const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "import.db"));
  db.exec("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, price REAL)");
  db.close();
  writeFileSync(join(dir, "items.csv"), "ID,Name,price,extra\n1,pen,1.5,x\n2,ink,0.25,y\n3,it's,10,z\n");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000021"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "import.db")}'
connection_type = "prod"
save_password = false
`,
  );
}

async function menu(page, label, item, modal = dialog) {
  await page.eval(`${row(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(item)})).click()`,
  );
  await page.waitFor(`${modal} !== null`);
}

/** Sets the dialog's React-controlled `input` to `value`, as typing would. */
const fill = (input, value) => `(() => {
  const el = ${input};
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('input', { bubbles: true }));
})()`;

/** Types the file path into the dialog and reads it, as Enter does. */
async function chooseFile(page, path) {
  await page.eval(fill(`${dialog}.querySelector('input')`, path));
  await page.waitFor(`${dialog}.querySelector('input').value === ${JSON.stringify(path)}`);
  await page.eval(`${dialog}.querySelector('input').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))`);
  await page.waitFor(`${dialog}.querySelectorAll('.import-preview tbody tr').length === 3`, 15_000);
}

/** Runs the dialog's import (`action` is its button) and waits until it is done. */
async function runImport(page, { action = "Import", production = true } = {}) {
  await page.waitFor(`!${button(dialog, action)}.disabled`);
  await page.eval(`${button(dialog, action)}.click()`);
  if (production) await confirmProduction(page);
  await page.waitFor(`${dialog}.textContent.includes('Imported 3 rows') || ${dialog}.textContent.includes('Nothing was imported')`, 20_000);
  const text = await page.eval(`${dialog}.textContent`);
  assert.ok(text.includes("Imported 3 rows"), text);
  await page.eval(`${button(dialog, "Close")}.click()`);
  await page.waitFor(`${dialog} === null`);
}

/** A production connection lists the changes first. */
async function confirmProduction(page) {
  await page.waitFor(`document.querySelector('.modal.guard')?.textContent.includes('INSERT INTO')`);
  await page.eval(`${button("document.querySelector('.modal.guard')", "Run")}.click()`);
}

export default async function (page, { dir }) {
  const csv = join(dir, "items.csv");
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("items")} !== undefined`, 15_000);

  // Into the existing table: columns match by name whatever the case; `extra` stays out.
  await menu(page, "items", "Import data");
  await chooseFile(page, csv);
  const mapped = await page.eval(`[...${dialog}.querySelectorAll('.import-mapping select')].map(s => s.value)`);
  assert.deepEqual(mapped, ["ID", "Name", "price"]);
  await runImport(page);

  // Again, replacing the rows: still three.
  await menu(page, "items", "Import data");
  await chooseFile(page, csv);
  await page.eval(`[...${dialog}.querySelectorAll('.import-modes label')].find(l => l.textContent.startsWith('Replace')).querySelector('input').click()`);
  await runImport(page);

  // Into a new table, with the types DuckDB inferred; the name it suggests, the file's, is taken.
  await menu(page, "main", "Import data into a new table");
  await chooseFile(page, csv);
  const types = await page.eval(`[...${dialog}.querySelectorAll('.import-mapping input:not([type])')].map(i => i.value)`);
  assert.deepEqual(types, ["ID", "INTEGER", "Name", "TEXT", "price", "REAL", "extra", "TEXT"]);
  await page.eval(fill(`${dialog}.querySelector('label.field:not(.grow) input')`, "items_copy"));
  await runImport(page);

  const db = new DatabaseSync(join(dir, "import.db"));
  assert.deepEqual(
    db.prepare("SELECT id, name, price FROM items ORDER BY id").all().map((r) => ({ ...r })),
    [
      { id: 1, name: "pen", price: 1.5 },
      { id: 2, name: "ink", price: 0.25 },
      { id: 3, name: "it's", price: 10 },
    ],
  );
  assert.equal(db.prepare("SELECT count(*) AS n FROM items_copy JOIN items USING (id) WHERE extra IS NOT NULL").get().n, 3);
  db.close();

  // A dropped CSV opens the new connection dialog for a DuckDB database on it.
  await page.eval(`window.__platypus.openDroppedFile(${JSON.stringify(csv)})`);
  await page.waitFor(`document.querySelector('.modal input')?.value === 'items.csv'`);
  await page.eval(`${button("document.querySelector('.modal')", "Save")}.click()`);
  await page.waitFor(`${row("items.csv")} !== undefined`);
  await page.eval(`${row("items.csv")}.querySelector('.twisty').click()`);
  // Its view, named after the file, sits next to the SQLite table of the same name.
  await page.waitFor(`${row("Views")} !== undefined`, 15_000);
  await page.eval(`${row("Views")}.querySelector('.twisty').click()`);
  await page.waitFor(`[...document.querySelectorAll('.tree-label')].filter(l => l.textContent === 'items').length === 2`, 15_000);

  // The same file dropped again, spelled in other case, opens a script on that connection.
  await page.eval(`window.__platypus.openDroppedFile(${JSON.stringify(csv.toUpperCase())})`);
  await page.waitFor(`document.querySelector('.tab.active') !== null && document.querySelector('.modal') === null`);

  // A copy of the SQLite table: the DuckDB database on the CSV lives in memory, so it is no
  // target, and the copy goes next to the table.
  await menu(page, "items", "Copy to a database");
  await page.waitFor(`${dialog}.querySelectorAll('.import-preview tbody tr').length === 3`, 15_000);
  const target = await page.eval(`${dialog}.querySelector('select').selectedOptions[0].textContent`);
  assert.equal(target, NAME);
  const options = await page.eval(`[...${dialog}.querySelector('select').options].map(o => o.textContent)`);
  assert.deepEqual(options, [NAME]);
  // `items_copy` was imported above.
  await page.eval(fill(`${dialog}.querySelector('input[list]')`, "items_copy2"));
  await page.waitFor(`[...${dialog}.querySelectorAll('label.field span')].some(s => s.textContent === 'New table')`);
  await runImport(page, { action: "Copy" });

  // Fake rows: keys after the largest, names and prices; the production guard lists them.
  const gen = `document.querySelector('.modal.generate-data')`;
  await menu(page, "items", "Generate data", gen);
  await page.waitFor(`${gen}.querySelectorAll('select').length === 3`);
  const kinds = await page.eval(`[...${gen}.querySelectorAll('select')].map(s => s.value)`);
  assert.deepEqual(kinds, ["next_key", "full_name", "decimal"]);
  await page.eval(fill(`${gen}.querySelector('input[type=number]')`, "50"));
  await page.eval(`${button(gen, "Generate")}.click()`);
  await confirmProduction(page);
  await page.waitFor(`${gen}.textContent.includes('Added 50 rows') || ${gen}.textContent.includes('Nothing was added')`, 20_000);
  assert.ok((await page.eval(`${gen}.textContent`)).includes("Added 50 rows"), await page.eval(`${gen}.textContent`));
  await page.eval(`${button(gen, "Close")}.click()`);
  const after = new DatabaseSync(join(dir, "import.db"));
  assert.deepEqual({ ...after.prepare("SELECT count(*) AS n, max(id) AS top FROM items").get() }, { n: 53, top: 53 });
  after.close();
}
