// UX P3 fixes in the workbench (SQLite):
// - the navigator's table menu keeps "Copy name" and "Copy qualified name" in a "Copy" submenu;
// - the data editor's bar says what kinds of change are pending ("1 new, 1 deleted"), and the
//   preview's default button is "Cancel", not "Close";
// - the object page leaves out an empty "Details" column of the foreign keys table;
// - the table editor's "Save…" says why it is off, and the revert button is "Discard";
// - the result SQL field has the short placeholder; a failed query offers "Copy error".
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { revealItem, submenuItems } from "../menu-sub.mjs";
import { runQuery, treeRow, visibleTab } from "../query.mjs";

const NAME = "UX workbench";
const menu = `document.querySelector('.context-menu')`;
const topLabels = `[...${menu}.children].map(c => (c.matches('button') ? c : c.querySelector(':scope > button'))?.querySelector('span')?.textContent).filter(Boolean)`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar:not([hidden])')?.textContent ?? '')`;
const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const button = (scope, label) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "ux.db"));
  db.exec(`CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT);
    CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent(id), v TEXT);
    INSERT INTO parent VALUES (1, 'a'), (2, 'b');
    INSERT INTO child VALUES (1, 1, 'x'), (2, 2, 'y');`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000a31"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "ux.db")}'
save_password = false
`,
  );
}

async function rightClickRow(page, label) {
  await page.eval(`${treeRow(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`${menu} !== null`);
}

const clickMenu = (page, label) =>
  page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)}).click()`);

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("child")} !== undefined`, 15_000);

  // The table menu: one "Copy" submenu holds both copies; bookmarks stay on the first level.
  await rightClickRow(page, "child");
  const top = await page.eval(topLabels);
  assert.ok(top.includes("Copy"), `Copy in ${top}`);
  assert.ok(!top.includes("Copy name") && !top.includes("Copy qualified name"), `copies are in the submenu: ${top}`);
  const copies = await submenuItems(page, "Copy");
  assert.ok(copies.includes("Copy name") && copies.includes("Copy qualified name"), `${copies}`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${menu} === null`);

  // Object page, Foreign keys: the "Details" column is there only when a row has something.
  await rightClickRow(page, "child");
  assert.ok(await revealItem(page, "Open details"));
  await clickMenu(page, "Open details");
  await page.waitFor(`document.querySelector('.object-sections') != null`, 15_000);
  await page.eval(`[...document.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith('Foreign keys')).click()`);
  await page.waitFor(`document.querySelector('.meta-table th') !== null && [...document.querySelectorAll('.meta-table th')].some(th => th.textContent === 'References')`);
  const details = await page.eval(`(() => {
    const ths = [...document.querySelectorAll('.meta-table th')].map(th => th.textContent);
    const at = ths.indexOf('Details');
    const filled = at >= 0 && [...document.querySelectorAll('.meta-table tbody tr')].some(tr => tr.children[at]?.textContent !== '');
    return { has: at >= 0, filled, cells: document.querySelector('.meta-table tbody tr').children.length, heads: ths.length };
  })()`);
  assert.equal(details.heads, details.cells, `a header per cell: ${JSON.stringify(details)}`);
  assert.equal(details.has, details.filled, `Details only when filled: ${JSON.stringify(details)}`);

  // Data editor: a new row and a deleted one, said by kind; the preview cancels by default.
  await page.eval(`${treeRow("child")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await page.eval(`${grid}.focus()`);
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("Delete", "Delete", 46);
  await page.waitFor(`${editBar}.includes('1 row changed')`);
  assert.ok(!(await page.eval(editBar)).includes("("), "one kind of change has no breakdown");
  await page.eval(`${visibleTab}.querySelector('.edit-bar button[aria-label="Add a row"]').click()`);
  await page.waitFor(`${editBar}.includes('2 rows changed (1 new, 1 deleted)')`);
  await page.eval(`${visibleTab}.querySelector('.edit-bar button[aria-label="Preview SQL"]').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') !== null`);
  assert.equal(await page.eval(`document.activeElement?.textContent`), "Cancel", "Cancel has the focus");
  await page.eval(`${button("document.querySelector('.modal')", "Cancel")}.click()`);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
  await page.eval(`${visibleTab}.querySelector('.edit-bar button[aria-label="Discard"]').click()`);
  await page.waitFor(`${editBar}.includes('to edit a cell')`);

  // Result SQL field: the short placeholder.
  assert.ok(
    (await page.eval(`${visibleTab}.querySelector('.result-sql-field .cm-placeholder')?.textContent ?? ''`)).startsWith("Filter these rows with SQL"),
  );

  // A failed query: the error panel offers Copy error.
  await runQuery(page, "SELECT FROM");
  await page.waitFor(`${visibleTab}.querySelector('.result-error') !== null`, 15_000);
  assert.ok(await page.eval(`${button(`${visibleTab}.querySelector('.result-error')`, "Copy error")} !== undefined`));
}
