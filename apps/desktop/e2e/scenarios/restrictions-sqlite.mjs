// Restrictions of a connection: one whose data is not exported offers no export, dump or
// copy to another database, and its result has no Export…; one whose rows change only by SQL
// offers no import or generated data, and its table opens read-only in the grid.
import assert from "node:assert/strict";
import { openSubmenu } from "../menu-sub.mjs";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const rows = (label) => `[...document.querySelectorAll('.tree-row')].filter(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const row = (label) => `${rows(label)}[0]`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;
const menuItems = `[...document.querySelectorAll('.context-menu button')].map(b => b.textContent)`;

export function setup({ dir }) {
  for (const file of ["kept.db", "sql.db"]) {
    const db = new DatabaseSync(join(dir, file));
    db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'a'), (2, 'b');");
    db.close();
  }
  const c = (id, name, file, restriction) => `
[[connections]]
id = "00000000-0000-4000-8000-00000000081${id}"
name = "${name}"
driver = "sqlite"
database = '${join(dir, file)}'
save_password = false

[connections.restrictions]
${restriction} = true
`;
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${c(1, "Kept db", "kept.db", "no_export")}${c(2, "Sql db", "sql.db", "no_data_editing")}`);
}

/** The items of the context menu of the `index`-th navigator row `label`; the menu closes. */
async function menuOf(page, label, index = 0) {
  await page.eval(`${rows(label)}[${index}].querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  // Import, export, copy and generated data live in the Data submenu: open it to list them.
  await openSubmenu(page, "Data");
  const items = await page.eval(menuItems);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
  return items;
}

const has = (items, text) => items.some((i) => i.startsWith(text));

async function openTable(page, index) {
  await page.eval(`${rows("t")}[${index}].id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${status} === '2 rows'`, 15_000);
}

export default async function (page, { dir }) {
  for (const name of ["Kept db", "Sql db"]) {
    await page.waitFor(`${row(name)} !== undefined`);
    await page.eval(`${row(name)}.querySelector('.twisty').click()`);
  }
  await page.waitFor(`${rows("t")}.length === 2`, 15_000);

  // No exports: the data stays in.
  const kept = await menuOf(page, "t", 0);
  for (const gone of ["Export data", "Copy to a database", "Dump to SQL file"]) assert.ok(!has(kept, gone), `${gone} in ${kept}`);
  assert.ok(has(kept, "Import data") && has(kept, "Generate data"), `${kept}`);
  const keptSchema = await menuOf(page, "main", 0);
  for (const gone of ["Export all tables", "Copy schema to a database", "Dump to SQL file"]) assert.ok(!has(keptSchema, gone), `${gone} in ${keptSchema}`);
  await openTable(page, 0);
  assert.equal(await page.eval(`[...${visibleTab}.querySelectorAll('button')].some(b => b.textContent === 'Export…')`), false);
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);

  // Rows only by SQL: no imports nor generated rows, and a read-only grid.
  const sql = await menuOf(page, "t", 1);
  for (const gone of ["Import data", "Generate data"]) assert.ok(!has(sql, gone), `${gone} in ${sql}`);
  assert.ok(has(sql, "Export data") && has(sql, "Copy to a database"), `${sql}`);
  await openTable(page, 1);
  await page.waitFor(`${editBar}.includes('the connection changes its rows only by SQL')`, 15_000);
  assert.ok(await page.eval(`[...${visibleTab}.querySelectorAll('button')].some(b => b.textContent === 'Export…')`));

  const db = new DatabaseSync(join(dir, "sql.db"));
  assert.equal(db.prepare("SELECT count(*) AS n FROM t").get().n, 2);
  db.close();
}
