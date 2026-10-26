// Charts of a result: the switch from the grid, bars from the first text and number columns,
// one Y column less and a pie, the chart kept when the statement runs again, and the export to
// PNG and SVG files.
import assert from "node:assert/strict";
import { existsSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { openTable, rerun, runQuery, sqliteConnection, status, visibleTab } from "../query.mjs";

const NAME = "E2E charts";
const chart = `${visibleTab}?.querySelector('.chart-svg')`;
const toggle = (label) => `[...${visibleTab}.querySelectorAll('.view-toggle button')].find(b => b.textContent === ${JSON.stringify(label)})`;
const yBox = (column) =>
  `[...${visibleTab}.querySelectorAll('.chart-side fieldset label')].find(l => l.textContent === ${JSON.stringify(column)}).querySelector('input')`;
const exportButton = (label) =>
  `[...${visibleTab}.querySelectorAll('.chart-export button')].find(b => b.textContent === ${JSON.stringify(label)})`;

/** Sets a select of the chart's side panel as a user would. */
const choose = (label, value) => `(() => {
  const el = ${visibleTab}.querySelector('select[aria-label=${JSON.stringify(label)}]');
  Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('change', { bubbles: true }));
})()`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "charts.db",
    id: "00000000-0000-4000-8000-000000000032",
    name: NAME,
    sql: `CREATE TABLE sales (id INTEGER PRIMARY KEY, region TEXT, amount REAL);
           INSERT INTO sales VALUES (1, 'north', 10), (2, 'south', 5), (3, 'north', 7), (4, 'east', 3);`,
  });
}

/** Waits for the app to write `path`. */
async function written(page, path) {
  for (let i = 0; i < 100 && !(existsSync(path) && statSync(path).size > 0); i++) await page.sleep(100);
  assert.ok(existsSync(path), `${path} written`);
}

export default async function (page, { dir }) {
  await openTable(page, NAME, "sales", "4 rows");
  await runQuery(page, "SELECT region, sum(amount) AS total, count(*) AS orders FROM sales GROUP BY region ORDER BY region");
  assert.equal(await page.eval(status), "3 rows");

  // Bars of total and orders by region: the first text column across, the numbers up.
  assert.equal(await page.eval(`${chart} ?? null`), null);
  await page.eval(`${toggle("Chart")}.click()`);
  await page.waitFor(`${chart}?.querySelectorAll('rect.chart-bar').length === 6`, 10_000);
  await page.eval(`${yBox("orders")}.click()`);
  await page.waitFor(`${chart}?.querySelectorAll('rect.chart-bar').length === 3`);
  await page.eval(choose("Chart type", "pie"));
  await page.waitFor(`${chart}?.querySelectorAll('path.chart-slice').length === 3`);

  // Running the statement again keeps its chart.
  await page.eval(`${chart}.dataset.old = '1'`);
  await rerun(page);
  await page.waitFor(
    `${chart} != null && ${chart}.dataset.old === undefined && ${chart}.querySelectorAll('path.chart-slice').length === 3`,
    10_000,
  );

  // Export to the files the save picker answers.
  const png = join(dir, "chart.png");
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(png)})`);
  await page.eval(`${exportButton("PNG image…")}.click()`);
  await written(page, png);
  assert.deepEqual([...readFileSync(png).subarray(0, 4)], [0x89, 0x50, 0x4e, 0x47]);
  const svg = join(dir, "chart.svg");
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(svg)})`);
  await page.eval(`${exportButton("SVG image…")}.click()`);
  await written(page, svg);
  const text = readFileSync(svg, "utf8");
  assert.ok(text.startsWith("<?xml"));
  assert.equal(text.match(/class="chart-slice"/g)?.length, 3);

  // Back to the grid.
  await page.eval(`${toggle("Grid")}.click()`);
  await page.waitFor(`${chart} == null && ${visibleTab}.querySelector('.grid-scroll') != null`);
}
