// More of the read-only grid on a throwaway SQLite database: the column layout (hidden and
// moved columns, kept for the table), totals of the selection, counts by value, the record
// view, foreign keys as links with the References panel, column metadata, the calc panel,
// two rows compared, auto-refresh, number formats, and geometries on the map.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { ALT, CTRL, SHIFT } from "../cdp.mjs";

const NAME = "E2E grid more";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const menuButton = (label) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)})`;
const button = (scope, label) =>
  `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const path = join(dir, "more.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT, city TEXT);
           INSERT INTO customers VALUES (1, 'Ada', 'Rome'), (2, 'Bob', 'Oslo'), (3, 'Cy', 'Rome');
           CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), total REAL);
           INSERT INTO orders VALUES (10, 1, 1234.5), (11, 1, 2), (12, 2, 3.25);
           CREATE TABLE places (id INTEGER PRIMARY KEY, shape TEXT, wkb BLOB);
           INSERT INTO places VALUES
             (1, 'POINT (1 2)', X'010100000000000000000014400000000000001440'),
             (2, 'POLYGON ((0 0, 4 0, 4 4, 0 4, 0 0))', NULL),
             (3, 'LINESTRING (0 0, 3 3)', NULL),
             (4, 'hello', NULL);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000005"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

async function openTable(page, name, rows) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`${treeRow(name)}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${status} === '${rows}'`, 15_000);
  await markGrid(page);
}

/** Names the visible tab's grid `e2e-grid`; hidden tabs keep theirs otherwise. */
async function markGrid(page) {
  await page.eval(`document.querySelectorAll('#e2e-grid').forEach(e => e.removeAttribute('id'))`);
  await page.eval(`${grid}.id = 'e2e-grid'`);
}

/** Focuses the cell `col` display positions right of the first column of `row`. */
async function focusCell(page, row, col) {
  await page.eval(`document.getElementById('e2e-grid').focus()`);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < col; i++) await page.key("ArrowRight", "ArrowRight", 39);
}

/** The grid's menu for the focused cell: opened on the row numbers, which keep the focus. */
async function openMenu(page) {
  const r = await page.rect(`#e2e-grid`);
  await page.click(r.x + 4, r.y + 52, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
}

async function menu(page, ...path) {
  for (const label of path) {
    await page.waitFor(`${menuButton(label)} !== undefined`);
    await page.eval(`${menuButton(label)}.click()`);
  }
}

async function copyAll(page) {
  await page.eval(`window.__copied = undefined`);
  await page.eval(`document.getElementById('e2e-grid').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.key("c", "KeyC", 67, CTRL);
  await page.waitFor(`window.__copied !== undefined`);
  return page.eval(`window.__copied`);
}

async function closeTab(page) {
  await page.key("w", "KeyW", 87, CTRL);
}

/** On `orders`: the focused column's metadata, statistics of a selection, two rows compared. */
async function panels(page, shot) {
  const panelText = (selector) => `(document.querySelector(${JSON.stringify(selector)})?.textContent ?? '')`;
  await focusCell(page, 0, 2);
  await openMenu(page);
  await menu(page, "Column metadata");
  await page.waitFor(`${panelText(".metadata-list")}.includes('Tablemain.orders')`);
  const metadata = await page.eval(panelText(".metadata-list"));
  for (const part of ["TypeREAL", "Columntotal", "Nullableyes", "Primary keyno", "Row keyno"]) {
    assert.ok(metadata.includes(part), `${part} in ${metadata}`);
  }
  await page.eval(`${button("document.querySelector('.metadata-panel')", "×")}.click()`);

  // The calc panel follows the selection: all of `total`.
  await openMenu(page);
  await menu(page, "Calculate");
  await page.waitFor(`document.querySelector('.calc-panel') !== null`);
  await page.eval(`document.getElementById('e2e-grid').focus()`);
  await page.key("ArrowDown", "ArrowDown", 40, SHIFT);
  await page.key("ArrowDown", "ArrowDown", 40, SHIFT);
  await page.waitFor(`${panelText(".calc-panel .value-size")} === '3 rows selected'`);
  const calc = await page.eval(panelText(".calc-list"));
  assert.match(calc, /Count3NULL0Distinct3Sum1[.,]?239[.,]75/);
  assert.match(calc, /Median3[.,]25/);
  await shot("calc");
  await page.eval(`${button("document.querySelector('.calc-panel')", "×")}.click()`);

  // Two rows selected: compared column by column.
  await focusCell(page, 0, 0);
  await page.key("ArrowDown", "ArrowDown", 40, SHIFT);
  await openMenu(page);
  await menu(page, "Compare the two rows");
  await page.waitFor(`document.querySelectorAll('.compare-list tbody tr').length === 3`);
  assert.deepEqual(
    await page.eval(`[...document.querySelectorAll('.compare-list tbody tr.differs th')].map(t => t.textContent)`),
    ["id", "total"],
  );
  await shot("compare");
  await page.eval(`${button("document.querySelector('.compare-rows')", "Close")}.click()`);
  await page.waitFor(`document.querySelector('.compare-rows') === null`);
}

/** Auto-refresh runs the table's query again: a row added outside the app shows up. */
async function autoRefresh(page, path) {
  const select = `${visibleTab}.querySelector('.auto-refresh select')`;
  const choose = (seconds) =>
    page.eval(`(() => { const s = ${select}; s.value = '${seconds}'; s.dispatchEvent(new Event('change', { bubbles: true })); })()`);
  await choose(2);
  const db = new DatabaseSync(path);
  db.exec("INSERT INTO orders VALUES (13, 3, 1)");
  await page.waitFor(`${status} === '4 rows'`, 15_000);
  await choose(0);
  db.exec("DELETE FROM orders WHERE id = 13");
  db.close();
  // Off: the deleted row stays until the next run.
  await page.sleep(2500);
  assert.equal(await page.eval(status), "4 rows");
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`${visibleTab}.querySelector('.toolbar button.primary').click()`);
  await page.waitFor(`${status} === '3 rows'`, 15_000);
  await markGrid(page);
}

/** On `places`: WKT text and WKB blobs drawn on the map; a click focuses the shape's row. */
async function map(page, shot) {
  const size = `(document.querySelector('.map-panel .value-size')?.textContent ?? '')`;
  await focusCell(page, 0, 1);
  await openMenu(page);
  await menu(page, "Show on map");
  await page.waitFor(`${size} === '3 geometries · 1 not readable'`);
  // The WKB point of the next column joins in.
  await page.eval(`document.getElementById('e2e-grid').focus()`);
  await page.key("ArrowRight", "ArrowRight", 39, SHIFT);
  await page.waitFor(`${size} === '4 geometries · 1 not readable'`);
  await page.waitFor(`document.querySelector('.map-shape.focused')?.dataset.row === '0'`);
  await shot("map");
  await page.eval(`document.querySelector('.map-shape[data-row="2"]').id = 'e2e-line'`);
  const line = await page.rect("#e2e-line");
  await page.click(line.x + line.w / 2, line.y + line.h / 2);
  await page.waitFor(`document.querySelector('.map-shape.focused')?.dataset.row === '2'`);
  await page.eval(`${button("document.querySelector('.map-panel')", "×")}.click()`);
}

export default async function (page, { shots, dir }) {
  const shot = async (name) => shots && (await page.screenshot(join(shots, `grid-more-${name}.png`)));
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);

  // Hide `city`, move `name` first in the Columns dialog.
  await openTable(page, "customers", "3 rows");
  await focusCell(page, 0, 2);
  await openMenu(page);
  await menu(page, "Columns", "Hide column");
  await openMenu(page);
  await menu(page, "Columns", "Columns…");
  await page.waitFor(`document.querySelector('.columns-list') !== null`);
  await page.eval(
    `[...document.querySelectorAll('.columns-row')].find(r => r.textContent.includes('name')).querySelector('button[title="Move left"]').click()`,
  );
  await page.eval(`${button("document.querySelector('.modal')", "Close")}.click()`);
  assert.equal(await copyAll(page), "Ada\t1\nBob\t2\nCy\t3");

  // The selection's totals: three values of a number column.
  await page.waitFor(`document.querySelector('.grid-summary')?.textContent.includes('Sum 6')`);
  await shot("layout");

  // The layout stays with the table.
  await closeTab(page);
  await openTable(page, "customers", "3 rows");
  assert.equal(await copyAll(page), "Ada\t1\nBob\t2\nCy\t3");
  await openMenu(page);
  await menu(page, "Columns", "Reset columns");
  assert.equal(await copyAll(page), "1\tAda\tRome\n2\tBob\tOslo\n3\tCy\tRome");

  // A pinned column goes first, and back when unpinned.
  await focusCell(page, 0, 2);
  await openMenu(page);
  await menu(page, "Columns", "Pin column");
  assert.equal(await copyAll(page), "Rome\t1\tAda\nOslo\t2\tBob\nRome\t3\tCy");
  await focusCell(page, 0, 0);
  await openMenu(page);
  await menu(page, "Columns", "Unpin column");
  assert.equal(await copyAll(page), "1\tAda\tRome\n2\tBob\tOslo\n3\tCy\tRome");

  // Dragging a header moves its column: `id` past the last one.
  const r = await page.rect("#e2e-grid");
  await page.mouse("mousePressed", r.x + 36, r.y + 20);
  await page.mouse("mouseMoved", r.x + 120, r.y + 20);
  await page.mouse("mouseMoved", r.x + 600, r.y + 20);
  await page.mouse("mouseReleased", r.x + 600, r.y + 20);
  assert.equal(await copyAll(page), "Ada\tRome\t1\nBob\tOslo\t2\nCy\tRome\t3");
  await openMenu(page);
  await menu(page, "Columns", "Reset columns");

  // Counts by value of the focused column.
  await focusCell(page, 0, 2);
  await openMenu(page);
  await menu(page, "Count by value");
  await page.waitFor(`document.querySelectorAll('.grouping-list tbody tr').length === 2`);
  assert.deepEqual(
    await page.eval(`[...document.querySelectorAll('.grouping-list tbody tr')].map(r => r.textContent)`),
    ["Rome2", "Oslo1"],
  );
  await shot("grouping");
  await page.eval(`${button("document.querySelector('.grouping-panel')", "×")}.click()`);

  // The record view shows one row and moves with the arrows.
  await page.eval(`document.getElementById('e2e-grid').focus()`);
  await page.key("Home", "Home", 36, CTRL);
  await page.key("r", "KeyR", 82, CTRL | ALT);
  await page.waitFor(`document.querySelector('.record-view .record-position')?.textContent === 'Row 1 of 3'`);
  await page.key("ArrowRight", "ArrowRight", 39);
  await page.waitFor(`document.querySelector('.record-position')?.textContent === 'Row 2 of 3'`);
  await page.waitFor(`[...document.querySelectorAll('.record-value')].map(v => v.textContent).join() === '2,Bob,Oslo'`);
  await shot("record");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.record-view') === null`);

  // The References panel counts the orders of the focused customer and opens them.
  await focusCell(page, 0, 0);
  await openMenu(page);
  await menu(page, "References panel");
  await page.waitFor(`document.querySelector('.reference button')?.textContent === '2 rows'`);
  await shot("references");
  await page.eval(`document.querySelector('.reference button').click()`);
  await page.waitFor(`${status} === '2 rows'`, 15_000);
  assert.match(await page.eval(`${visibleTab}.querySelector('.cm-content').textContent`), /WHERE customer_id = 1$/);
  await closeTab(page);

  // A foreign key opens the row it points to.
  await openTable(page, "orders", "3 rows");
  await focusCell(page, 0, 1);
  await openMenu(page);
  await menu(page, "Open referenced row");
  await page.waitFor(`${status} === '1 row'`, 15_000);
  assert.match(await page.eval(`${visibleTab}.querySelector('.cm-content').textContent`), /WHERE id = 1$/);
  await closeTab(page);

  await page.waitFor(`${status} === '3 rows'`);
  await markGrid(page);
  await panels(page, shot);
  await autoRefresh(page, join(dir, "more.db"));

  // Numbers grouped as set in the Settings; the grid fetches its rows again.
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Results");
  await page.eval(`(() => {
    const select = [...document.querySelectorAll('.setting')].find(s => s.textContent.includes('Numbers')).querySelector('select');
    select.value = 'comma';
    select.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
  await markGrid(page);
  // The grid shows 1,234.5; copying keeps the plain values, which paste back into the grid
  // and into other programs whatever the display format.
  assert.equal(await copyAll(page), "10\t1\t1234.5\n11\t1\t2.0\n12\t2\t3.25");

  await closeTab(page);
  await openTable(page, "places", "4 rows");
  await map(page, shot);
}
