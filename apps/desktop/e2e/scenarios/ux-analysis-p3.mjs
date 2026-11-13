// UX review, third pass, on the analysis tools (SQLite):
// - the query builder: a hint to add another table while there are fewer than three, a join
//   label with its own box between the two tables, the WHERE/SQL/buttons in one padded panel;
// - the notebook: the first SQL cell has no "on results" box, Save and Export have a border;
// - a pie of nine categories gives nine different colours, the chart's Export buttons come
//   after its options; the Pivot button says why it is off, and the status says the pivot.
import assert from "node:assert/strict";
import { choose, liteConnection, mark, newEditor, runQuery, status, toggle, visibleTab } from "../analysis-helpers.mjs";
import { expand, menu } from "../transfer-helpers.mjs";
import { treeMenu } from "../grid-helpers.mjs";

const NAME = "UX analysis p3";
const builder = `document.querySelector('.builder-tab:not([style*="none"])')`;
const builderSql = `(${builder}?.querySelector('.builder-sql')?.textContent ?? '')`;
const source = (name) => `[...${builder}.querySelectorAll('.builder-source')].find(r => r.textContent === ${JSON.stringify(name)})`;
const box = (name) => `${builder}.querySelector('.builder-box[data-table=${JSON.stringify(name)}]')`;
const tab = `[...document.querySelectorAll('.notebook-tab')].find(t => t.style.display !== 'none')`;
const cell = (name) => `${tab}?.querySelector('[data-name=${JSON.stringify(name)}]')`;
const pivotDialog = `document.querySelector('.pivot-dialog')`;
const pivotButton = `[...${pivotDialog}.querySelectorAll('.actions button')].find(b => b.textContent === 'Pivot')`;
const rowBox = (column) => `[...${pivotDialog}.querySelectorAll('fieldset label')].find(l => l.textContent === ${JSON.stringify(column)}).querySelector('input')`;

export function setup({ dir }) {
  const regions = ["Napoli", "Udine", "Genova", "Trieste", "Torino", "Roma", "Milano", "Bari", "Parma"];
  liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000b0301",
    `CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
     CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), total REAL);
     CREATE TABLE sales (id INTEGER PRIMARY KEY, city TEXT, amount REAL);
     INSERT INTO sales (city, amount) VALUES ${regions.map((r, i) => `('${r}', ${10 + i})`).join(", ")};`,
  );
}

async function queryBuilder(page) {
  await expand(page, NAME, "customers");
  await menu(page, "customers", "Visual query builder", null, NAME);
  await page.waitFor(`${box("customers")} !== null`, 15_000);
  // One table: the hint says how to add another.
  assert.match(await page.eval(`${builder}.querySelector('.builder-hint')?.textContent ?? ''`), /Drag another table/);
  await page.eval(`${source("orders")}.querySelector('.builder-add').click()`);
  await page.waitFor(`${builderSql}.includes('JOIN orders o')`, 10_000);
  await page.waitFor(`${builder}.querySelector('.builder-join-on') !== null`, 10_000);
  // The join's label is a box of its own, in the room between the two tables.
  const rects = await page.eval(`(() => {
    const r = (el) => el.getBoundingClientRect();
    const on = r(${builder}.querySelector('.builder-join-on'));
    return { left: r(${box("customers")}).right, right: r(${box("orders")}).left, on: { l: on.left, r: on.right }, border: getComputedStyle(${builder}.querySelector('.builder-join-on')).borderTopWidth };
  })()`);
  assert.ok(rects.right - rects.left >= 60, `room between the tables: ${JSON.stringify(rects)}`);
  assert.equal(rects.border, "1px");
  // A third table takes the hint away; the bottom panel is padded and holds the buttons.
  await page.eval(`${source("sales")}.querySelector('.builder-add').click()`);
  await page.waitFor(`${box("sales")} !== null`);
  assert.equal(await page.eval(`${builder}.querySelector('.builder-hint')`), null);
  assert.equal(await page.eval(`getComputedStyle(${builder}.querySelector('.builder-bottom')).paddingTop`), "12px");
  assert.equal(await page.eval(`${builder}.querySelector('.builder-bottom .actions .spacer')`), null);
}

async function notebook(page) {
  await treeMenu(page, NAME, "New notebook");
  await page.waitFor(`${cell("c1")} != null`);
  // The first cell has no cell before it to query; the second one has.
  assert.equal(await page.eval(`${cell("c1")}.querySelector('label.check')`), null);
  await page.eval(`${tab}.querySelector('[data-action="add_sql"]').click()`);
  await page.waitFor(`${cell("c2")}?.querySelector('label.check') != null`);
  assert.equal(await page.eval(`${cell("c2")}.querySelector('label.check').textContent.trim()`), "On results");
  // Save and Export are buttons with a border; the Save one names its shortcut.
  for (const action of ["save", "export_html"]) {
    const color = await page.eval(`getComputedStyle(${tab}.querySelector('[data-action=${JSON.stringify(action)}]')).borderTopColor`);
    assert.notEqual(color, "rgba(0, 0, 0, 0)", `${action} has a border`);
  }
  assert.match(await page.eval(`${tab}.querySelector('[data-action="save"]').title`), /Ctrl\+S/);
}

async function chartAndPivot(page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT city, amount FROM sales ORDER BY id");
  assert.equal(await page.eval(status), "9 rows");
  await page.eval(`${toggle("Chart")}.click()`);
  await page.waitFor(`${visibleTab}.querySelectorAll('rect.chart-bar').length === 9`, 10_000);
  await page.eval(choose("Chart type", "pie"));
  await page.waitFor(`${visibleTab}.querySelectorAll('path.chart-slice').length === 9`);
  // Nine slices, nine colours.
  assert.equal(await page.eval(`new Set([...${visibleTab}.querySelectorAll('path.chart-slice')].map(p => p.getAttribute('fill'))).size`), 9);
  // The Export buttons are the last of the options.
  assert.equal(await page.eval(`${visibleTab}.querySelector('.chart-side').lastElementChild.classList.contains('chart-export')`), true);
  await page.eval(`${toggle("Grid")}.click()`);
  await page.waitFor(`${visibleTab}.querySelector('.chart-svg') == null && ${visibleTab}.querySelector('.grid-scroll') != null`);

  // Pivot: off, with the reason, until a row field is chosen; then the status names the pivot.
  await page.eval(`${visibleTab}.querySelector('.result-pivot-open').click()`);
  await page.waitFor(`${pivotDialog} != null`);
  assert.equal(await page.eval(`${pivotButton}.disabled`), true);
  assert.equal(await page.eval(`${pivotButton}.title`), "Choose at least one row field");
  await page.eval(`${rowBox("city")}.click()`);
  await page.waitFor(`${pivotButton}.disabled === false`);
  assert.equal(await page.eval(`${pivotButton}.title`), "");
  await page.eval(`${pivotButton}.click()`);
  await page.waitFor(`${pivotDialog} == null && ${mark}.startsWith('SQL view')`, 30_000);
  assert.match(await page.eval(mark), /Pivot: city · count/);
}

export default async function (page) {
  await queryBuilder(page);
  await notebook(page);
  await chartAndPivot(page);
}
