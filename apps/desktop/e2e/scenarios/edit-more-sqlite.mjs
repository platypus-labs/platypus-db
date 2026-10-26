// More of the data editor on a throwaway SQLite database: the list of a foreign key's rows,
// rows pasted from a spreadsheet, the undo script of a save, a query's result edited like a
// table, a virtual key for a table without one, mass edits of a selection and cells loaded
// from files. Pasting goes through the system clipboard, which the scenario overwrites.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { ALT, CTRL, SHIFT } from "../cdp.mjs";

const NAME = "E2E edit more";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const statusBar = `(${visibleTab}?.querySelector('.statusbar')?.textContent ?? '')`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar:not([hidden])')?.textContent ?? '')`;
const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const button = (scope, label) =>
  `[...${scope}.querySelectorAll('button')].find(b => b.textContent === ${JSON.stringify(label)} || b.getAttribute('aria-label') === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const path = join(dir, "more.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
           INSERT INTO customers VALUES (1, 'Ada'), (2, 'Bob');
           CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), total REAL);
           INSERT INTO orders VALUES (10, 1, 5.5), (11, 2, 7);
           CREATE TABLE log (at TEXT, msg TEXT);
           INSERT INTO log VALUES ('t1', 'a'), ('t2', 'b');
           CREATE TABLE docs (id INTEGER PRIMARY KEY, doc JSON);
           INSERT INTO docs VALUES (1, '{"a":1,"b":[1,2]}');
           CREATE TABLE stock (id INTEGER PRIMARY KEY, name TEXT, qty INTEGER, pos INTEGER, data BLOB, note TEXT);
           INSERT INTO stock VALUES (1, 'a', 5, 0, x'00', NULL), (2, 'b', 6, 0, NULL, NULL), (3, 'c', 7, 0, NULL, NULL);`);
  db.close();
  // Files the file picker's test answer points to.
  writeFileSync(join(dir, "bytes.bin"), Buffer.from([1, 2, 3]));
  writeFileSync(join(dir, "note.txt"), "héllo", "utf8");
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

const menuButton = (label) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)})`;

/** Selects rows `from..to` of column `col`, from the keyboard. */
async function selectDown(page, from, to, col) {
  await focusCell(page, from, col);
  for (let i = from; i < to; i++) await page.key("ArrowDown", "ArrowDown", 40, SHIFT);
}

/** Runs a grid menu item, through its submenus: the menu opens on the row numbers, which
 *  keep the selection. */
async function gridMenu(page, ...path) {
  await page.eval(`${grid}.id = 'e2e-grid'`);
  const r = await page.rect("#e2e-grid");
  await page.eval(`document.getElementById('e2e-grid').removeAttribute('id')`);
  await page.click(r.x + 4, r.y + 52, { button: "right" });
  for (const label of path) {
    await page.waitFor(`${menuButton(label)} !== undefined`);
    await page.eval(`${menuButton(label)}.click()`);
  }
}

/** Sets the `index`-th text field of the mass edit dialog, as typing would. */
async function setField(page, index, text) {
  await page.eval(`(() => {
    const el = document.querySelectorAll('.mass-edit input:not([type])')[${index}];
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(text)});
    el.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
}

const summary = `(document.querySelector('.mass-edit-summary')?.textContent ?? '')`;

/** Waits until the dialog's summary is the one worked out from `input` (its fields, as the
 *  dialog lists them in `data-input`) and reads `text`. The summary of the fields before the
 *  last ones were typed can read the same, so the text alone does not say the input arrived. */
async function summaryOf(page, input, text) {
  const current = `document.querySelector('.mass-edit-summary')?.dataset.input`;
  await page.waitFor(`${current} === ${JSON.stringify(JSON.stringify(input))} && ${summary}.includes(${JSON.stringify(text)})`);
}

/** The preview's SQL once it holds every one of `parts`: fill down and loading a file apply
 *  after reading values, with nothing else on screen to wait for. */
async function previewWith(page, parts) {
  for (let i = 0; ; i++) {
    const sql = await previewSql(page);
    await page.key("Escape", "Escape", 27);
    await page.waitFor(`document.querySelector('.sql-preview') === null`);
    if (parts.every((p) => sql.includes(p))) return sql;
    assert.ok(i < 20, `${parts.join(" / ")} in ${sql}`);
  }
}

async function applyMassEdit(page) {
  await page.eval(`${button("document.querySelector('.mass-edit')", "Apply")}.click()`);
  await page.waitFor(`document.querySelector('.mass-edit') === null`);
}

/** Set value, fill down, replace, a number sequence and two cells loaded from files, saved
 *  together. */
async function massEdits(page, dir) {
  await openTable(page, "stock");
  await page.waitFor(`${status} === '3 rows' && ${editBar}.includes('to edit a cell')`, 15_000);

  // One value in the three cells of `name`.
  await selectDown(page, 0, 2, 1);
  await gridMenu(page, "Edit selection", "Set value…");
  await page.waitFor(`document.querySelector('.mass-edit') !== null`);
  await setField(page, 0, "same");
  await summaryOf(page, ["value", "same"], "3 cells will change.");
  await applyMassEdit(page);
  await page.waitFor(`${editBar}.includes('3 rows changed')`);

  // Fill down `qty` from the first row: Ctrl+Alt+D (Ctrl+D duplicates rows).
  await selectDown(page, 0, 2, 2);
  await page.key("d", "KeyD", 68, CTRL | ALT);
  await previewWith(page, ["qty = 5 WHERE id = 3"]);

  // Replace in `name`, counting the replacements first.
  await selectDown(page, 0, 2, 1);
  await gridMenu(page, "Edit selection", "Replace text…");
  await page.waitFor(`document.querySelector('.mass-edit') !== null`);
  await setField(page, 0, "SAME");
  await setField(page, 1, "item");
  await summaryOf(page, ["SAME", "item", false, false], "3 replacements in 3 cells.");
  await applyMassEdit(page);

  // Numbers from 10 by 5 down `pos`.
  await selectDown(page, 0, 2, 3);
  await gridMenu(page, "Edit selection", "Number sequence…");
  await page.waitFor(`document.querySelector('.mass-edit') !== null`);
  await setField(page, 0, "10");
  await setField(page, 1, "5");
  await summaryOf(page, ["10", "5"], "3 cells will change.");
  await applyMassEdit(page);

  // Bytes into the BLOB of the first row, UTF-8 text into the note of the second.
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(join(dir, "bytes.bin"))})`);
  await focusCell(page, 0, 4);
  await gridMenu(page, "Load from file…");
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(join(dir, "note.txt"))})`);
  await focusCell(page, 1, 5);
  await gridMenu(page, "Load from file…");

  await previewWith(page, [
    "UPDATE main.stock SET name = 'item', pos = 10, data = X'010203' WHERE id = 1;",
    "UPDATE main.stock SET name = 'item', qty = 5, pos = 15, note = 'héllo' WHERE id = 2;",
    "UPDATE main.stock SET name = 'item', qty = 5, pos = 20 WHERE id = 3;",
  ]);
  await previewSql(page);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await saveFromPreview(page);
  await page.waitFor(`${status} === '3 rows' && ${statusBar}.includes('Undo script')`, 15_000);

  const db = new DatabaseSync(join(dir, "more.db"));
  const rows = db.prepare("SELECT id, name, qty, pos, hex(data) AS data, note FROM stock ORDER BY id").all();
  db.close();
  assert.deepEqual(
    rows.map((r) => ({ ...r })),
    [
      { id: 1, name: "item", qty: 5, pos: 10, data: "010203", note: null },
      { id: 2, name: "item", qty: 5, pos: 15, data: "", note: "héllo" },
      { id: 3, name: "item", qty: 5, pos: 20, data: "", note: null },
    ],
  );
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

  // Spreadsheet rows paste as new rows, in their order, saved by one statement.
  setClipboard("12\t2\t9.5\r\n13\t1\t1\r\n");
  await focusCell(page, 0, 0);
  await page.key("V", "KeyV", 86, CTRL | SHIFT);
  await page.waitFor(`${editBar}.includes('2 rows changed')`);
  assert.equal(
    await previewSql(page),
    "INSERT INTO main.orders (id, customer_id, total) VALUES (12, 2, 9.5), (13, 1, 1);",
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
      `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')?.disabled`,
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

  // Formatting JSON in the cell editor is only visual: applying it is not a change.
  await openTable(page, "docs");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await focusCell(page, 0, 1);
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-popup-text') !== null`);
  await page.eval(`${button("document.querySelector('.cell-popup')", "Format JSON")}.click()`);
  await page.waitFor(`document.querySelector('.cell-popup-text').value.includes('\\n')`);
  await page.eval(`${button("document.querySelector('.cell-popup')", "Apply")}.click()`);
  await page.waitFor(`document.querySelector('.cell-popup') === null`);
  assert.ok(!/changed/.test(await page.eval(editBar)), "formatting JSON marked the cell as changed");

  await massEdits(page, dir);
}
