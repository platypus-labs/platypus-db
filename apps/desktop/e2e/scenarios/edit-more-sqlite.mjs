// More of the data editor on a throwaway SQLite database: the list of a foreign key's rows,
// rows pasted from a spreadsheet, the undo script of a save, a query's result edited like a
// table, and a virtual key for a table without one. Pasting goes through the system
// clipboard, which the scenario overwrites.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, SHIFT } from "../cdp.mjs";

const NAME = "E2E edit more";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const statusBar = `(${visibleTab}?.querySelector('.statusbar')?.textContent ?? '')`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar:not([hidden])')?.textContent ?? '')`;
const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const button = (scope, label) =>
  `[...${scope}.querySelectorAll('button')].find(b => b.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const path = join(dir, "more.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
           INSERT INTO customers VALUES (1, 'Ada'), (2, 'Bob');
           CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), total REAL);
           INSERT INTO orders VALUES (10, 1, 5.5), (11, 2, 7);
           CREATE TABLE log (at TEXT, msg TEXT);
           INSERT INTO log VALUES ('t1', 'a'), ('t2', 'b');`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000004"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

function setClipboard(text) {
  const r = spawnSync("powershell", ["-NoProfile", "-Command", "Set-Clipboard -Value $env:E2E_CLIP"], {
    env: { ...process.env, E2E_CLIP: text },
  });
  assert.equal(r.status, 0, String(r.stderr));
}

async function openTable(page, name) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`${treeRow(name)}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${visibleTab}?.querySelector('.edit-bar') != null`, 15_000);
}

/** Focuses the grid's cell at `row`, `col`. */
async function focusCell(page, row, col) {
  await page.eval(`${grid}.focus()`);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < col; i++) await page.key("ArrowRight", "ArrowRight", 39);
}

async function previewSql(page) {
  await page.eval(`${button(visibleTab, "Preview SQL")}.click()`);
  await page.waitFor(`document.querySelector('.sql-preview') !== null`);
  return page.eval(`document.querySelector('.sql-preview').textContent`);
}

async function saveFromPreview(page) {
  await page.eval(`${button("document.querySelector('.modal')", "Save")}.click()`);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
}

export default async function (page, { dir, shots }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);

  // A foreign key edits with the rows it can point to, by their name.
  await openTable(page, "orders");
  await page.waitFor(`${status} === '2 rows' && ${editBar}.includes('to edit a cell')`, 15_000);
  await focusCell(page, 0, 1);
  if (shots) await page.screenshot(join(shots, "edit-more-orders.png"));
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelectorAll('datalist option').length === 2`);
  assert.deepEqual(await page.eval(`[...document.querySelectorAll('datalist option')].map(o => o.textContent)`), [
    "Ada",
    "Bob",
  ]);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);

  // Spreadsheet rows paste as new rows, in their order.
  setClipboard("12\t2\t9.5\r\n13\t1\t1\r\n");
  await focusCell(page, 0, 0);
  await page.key("V", "KeyV", 86, CTRL | SHIFT);
  await page.waitFor(`${editBar}.includes('2 rows changed')`);
  assert.equal(
    await previewSql(page),
    "INSERT INTO main.orders (id, customer_id, total) VALUES (12, 2, 9.5);\n" +
      "INSERT INTO main.orders (id, customer_id, total) VALUES (13, 1, 1);",
  );
  await saveFromPreview(page);
  await page.waitFor(`${status} === '4 rows' && ${statusBar}.includes('Undo script')`, 15_000);

  // The undo script deletes the new rows again.
  await page.eval(`${button(visibleTab, "Undo script")}.click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content')?.textContent.includes('DELETE')`);
  assert.equal(
    await page.eval(`${visibleTab}.querySelector('.cm-content').textContent`),
    "DELETE FROM main.orders WHERE id = 12;DELETE FROM main.orders WHERE id = 13;",
  );
  await page.eval(`${button(visibleTab, "Script")}.click()`);
  // Both result tabs appear when the script starts; Ctrl+Enter is ignored until it ends.
  await page.waitFor(
    `${visibleTab}.querySelectorAll('.result-tab').length === 2 && ` +
      `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.textContent.includes('Cancel'))?.disabled`,
    15_000,
  );

  // A query of one table edits like the table; computed columns stay read-only.
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  const query = "SELECT id, total * 2 AS dbl, total FROM orders ORDER BY id";
  await page.type(query);
  // On a slow runner Ctrl+Enter could otherwise run the script still in the editor.
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent === ${JSON.stringify(query)}`);
  await page.key("Escape", "Escape", 27); // close the completion list
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${status} === '2 rows' && ${editBar}.includes('to edit a cell')`, 15_000);
  await focusCell(page, 0, 1);
  await page.key("F2", "F2", 113);
  await page.waitFor(`${editBar}.includes('dbl is not a column of orders')`);
  await focusCell(page, 0, 2);
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor') !== null`);
  await page.type("6");
  await page.key("Enter", "Enter", 13);
  assert.equal(await previewSql(page), "UPDATE main.orders SET total = 6 WHERE id = 10;");
  await saveFromPreview(page);
  await page.waitFor(`${status} === '2 rows' && ${statusBar}.includes('Undo script')`, 15_000);

  // A virtual key makes a table without one editable; pasted cells change the rows below.
  await openTable(page, "log");
  await page.waitFor(`${editBar}.startsWith('Read-only')`, 15_000);
  await page.eval(`${button(visibleTab, "Set a virtual key…")}.click()`);
  await page.waitFor(`document.querySelector('.virtual-key-columns') !== null`);
  await page.eval(
    `[...document.querySelectorAll('.virtual-key-columns label')].find(l => l.textContent === 'at').querySelector('input').click()`,
  );
  await page.eval(`${button("document.querySelector('.modal')", "Save")}.click()`);
  await page.waitFor(`${editBar}.includes('Virtual key: at') && ${editBar}.includes('to edit a cell')`, 15_000);
  setClipboard("q\r\nr");
  await focusCell(page, 0, 1);
  await page.key("v", "KeyV", 86, CTRL);
  await page.waitFor(`${editBar}.includes('2 rows changed')`);
  assert.equal(
    await previewSql(page),
    "UPDATE main.log SET msg = 'q' WHERE \"at\" = 't1';\nUPDATE main.log SET msg = 'r' WHERE \"at\" = 't2';",
  );
  await page.key("Escape", "Escape", 27);

  const toml = readFileSync(join(dir, "connections.toml"), "utf8");
  assert.match(toml, /\[\[connections\.virtual_keys\]\]\s+schema = "main"\s+table = "log"\s+columns = \["at"\]/);
}
