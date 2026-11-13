// Grid edits and the checks before a run: saving edits never runs a statement the user
// declined at the guard; switching to another result, the chart or the terminal, or closing
// the result asks before dropping edits not saved; Format JSON keeps numbers as written.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { pressF2 } from "../grid-helpers.mjs";

const NAME = "E2E guards";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const modalButton = (text) => `[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const confirm = `document.querySelector('.modal.confirm')`;
const resultTab = (i) => `${visibleTab}.querySelectorAll('.result-tab')[${i}]`;
const JSON_DOC = '{"id":9007199254740993,"price":1.10}';

export function setup({ dir }) {
  const path = join(dir, "guards.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT);
           INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'c');
           CREATE TABLE docs (id INTEGER PRIMARY KEY, doc JSON);
           INSERT INTO docs VALUES (1, '${JSON_DOC}');`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000031"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

/** Empties the status line, so a wait sees the next run's status and not the last one's. */
const blankStatus = (page) =>
  page.eval(`(() => { const s = ${visibleTab}?.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);

async function typeSql(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27); // closes the completion list
}

/** Focuses the grid's cell at result row `row` (0-based) and column `col`. */
async function focusCell(page, row, col) {
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < col; i++) await page.key("ArrowRight", "ArrowRight", 39);
}

async function editCell(page, row, col, text) {
  await focusCell(page, row, col);
  await pressF2(page, ".cell-editor");
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(text);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
}

/** Answers the "Discard the changes…?" question with Cancel. */
async function keepEdits(page) {
  await page.waitFor(`${confirm}?.textContent.includes('Discard the changes to the data')`);
  await page.eval(`${modalButton("Cancel")}.click()`);
  await page.waitFor(`${confirm} === null`);
  assert.match(await page.eval(editBar), /1 row changed/);
}

async function openTable(page, name) {
  await page.waitFor(`${treeRow(name)} !== undefined`, 15_000);
  await page.eval(`${treeRow(name)}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${visibleTab}?.querySelector('.edit-bar') != null`, 15_000);
}

export default async function (page, { dir }) {
  const db = new DatabaseSync(join(dir, "guards.db"));
  const count = () => db.prepare("SELECT count(*) AS n FROM t").get().n;

  // A DELETE declined at the guard does not run when edits of the result still shown are
  // saved: the SELECT that produced the result runs again instead.
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  await typeSql(page, "SELECT * FROM t");
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${status} === '3 rows' && ${editBar}.includes('to edit a cell')`, 15_000);
  await typeSql(page, "DELETE FROM t");
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`document.querySelector('.modal.guard') !== null`, 15_000);
  await page.eval(`${modalButton("Cancel")}.click()`);
  await page.waitFor(`document.querySelector('.modal.guard') === null`);
  assert.equal(count(), 3);
  await editCell(page, 0, 1, "edited");
  await page.waitFor(`${editBar}.includes('1 row changed')`);
  await blankStatus(page);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("s", "KeyS", 83, CTRL);
  await page.waitFor(`${status} === '3 rows' && ${editBar}.includes('to edit a cell')`, 15_000);
  assert.equal(await page.eval(`document.querySelector('.modal.guard')`), null, "nothing to confirm: only the SELECT ran");
  assert.equal(count(), 3, "the declined DELETE did not run");
  assert.equal(db.prepare("SELECT v FROM t WHERE id = 1").get().v, "edited");

  // Data viewer with a second result appended: leaving the edited result asks first. The
  // connection opens in the navigator, its current schema with it.
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await openTable(page, "t");
  await page.waitFor(`${status} === '3 rows' && ${editBar}.includes('to edit a cell')`, 15_000);
  await typeSql(page, "SELECT 1 AS one");
  await page.key("\\", "Backslash", 220, CTRL);
  await page.waitFor(`${visibleTab}.querySelectorAll('.result-tab').length === 2 && ${status} === '1 row'`, 15_000);
  await page.eval(`${resultTab(0)}.click()`);
  await page.waitFor(`${resultTab(0)}.classList.contains('active') && ${editBar}.includes('to edit a cell')`);
  await editCell(page, 1, 1, "kept");
  await page.waitFor(`${editBar}.includes('1 row changed')`);

  // Another result tab.
  await page.eval(`${resultTab(1)}.click()`);
  await keepEdits(page);
  assert.equal(await page.eval(`${resultTab(0)}.classList.contains('active')`), true);
  // The chart.
  await page.eval(`[...${visibleTab}.querySelectorAll('.view-toggle button')].find(b => b.textContent === 'Chart').click()`);
  await keepEdits(page);
  assert.equal(await page.eval(`${visibleTab}.querySelector('.grid-scroll') !== null`), true);
  // Terminal mode.
  await page.eval(`${visibleTab}.querySelector('.toolbar button[aria-label="Terminal"]').click()`);
  await keepEdits(page);
  assert.equal(await page.eval(`${visibleTab}.querySelector('.toolbar button[aria-label="Terminal"]').classList.contains('toggled')`), false);
  // Closing the result.
  await page.eval(`${resultTab(0)}.querySelector('.result-tab-close').click()`);
  await keepEdits(page);
  assert.equal(await page.eval(`${visibleTab}.querySelectorAll('.result-tab').length`), 2);
  // Discarding goes on.
  await page.eval(`${resultTab(1)}.click()`);
  await page.waitFor(`${confirm} !== null`);
  await page.eval(`${modalButton("Discard")}.click()`);
  await page.waitFor(`${confirm} === null && ${resultTab(1)}.classList.contains('active')`);
  assert.equal(db.prepare("SELECT v FROM t WHERE id = 2").get().v, "b");

  // Format JSON only changes the layout: numbers keep their digits, and the cell is unchanged.
  await openTable(page, "docs");
  await page.waitFor(`${status} === '1 row' && ${editBar}.includes('to edit a cell')`, 15_000);
  await focusCell(page, 0, 1);
  await pressF2(page, ".cell-popup textarea");
  await page.eval(`[...document.querySelectorAll('.cell-popup button')].find(b => b.textContent === 'Format JSON').click()`);
  await page.waitFor(`document.querySelector('.cell-popup textarea').value.includes('\\n')`);
  const formatted = await page.eval(`document.querySelector('.cell-popup textarea').value`);
  assert.equal(formatted, '{\n  "id": 9007199254740993,\n  "price": 1.10\n}');
  await page.eval(`[...document.querySelectorAll('.cell-popup button')].find(b => b.textContent === 'Apply').click()`);
  await page.waitFor(`document.querySelector('.cell-popup') === null`);
  assert.match(await page.eval(editBar), /to edit a cell/, "formatting is not a change");
  db.close();
}
