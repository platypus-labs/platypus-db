// Group T (UX review) on SQLite, notebook and chart:
// - "Export HTML…" and "Save" confirm with a toast, and the exported chart keeps its titles;
// - a running cell shows an indicator and Stop; Stop ends the query, and Stop during "Run all"
//   keeps the next cells from starting;
// - a chart has a title and axis titles;
// - two series of very different scales get a right-hand axis, named in the legend.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { saveConnections } from "../perf.mjs";
import { treeMenu } from "../grid-helpers.mjs";

const NAME = "E2E UX Notebook";
const tab = `[...document.querySelectorAll('.notebook-tab')].find(t => t.style.display !== 'none')`;
const cell = (name) => `${tab}?.querySelector('[data-name=${JSON.stringify(name)}]')`;
const click = (scope, action) => `${scope}.querySelector('[data-action=${JSON.stringify(action)}]').click()`;
const toasts = `[...document.querySelectorAll('.toast')].map(t => t.textContent).join(' | ')`;
// A count of hundreds of millions of rows: it runs for minutes unless it is cancelled.
const SLOW = "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2000000000) SELECT count(*) FROM n";

export function setup({ dir }) {
  const file = join(dir, "orders.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE sales (id INTEGER PRIMARY KEY, region TEXT, amount INTEGER, orders INTEGER);
    INSERT INTO sales (region, amount, orders) VALUES ('North', 12000, 4), ('South', 8600, 3), ('East', 14300, 6), ('West', 5400, 2);`);
  db.close();
  saveConnections(
    dir,
    `[[connections]]\nid = "00000000-0000-4000-8000-0000000f0101"\nname = "${NAME}"\ndriver = "sqlite"\ndatabase = '${file}'\nsave_password = false\n`,
  );
}

async function typeIn(page, name, sql) {
  await page.eval(`${cell(name)}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`[...${cell(name)}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);
  // The completion list would take the next keys.
  await page.key("Escape", "Escape", 27);
}

async function addSql(page, sql) {
  const before = await page.eval(`${tab}.querySelectorAll('.notebook-cell.sql').length`);
  await page.eval(click(tab, "add_sql"));
  const name = `c${before + 1}`;
  await page.waitFor(`${cell(name)}?.querySelector('.cm-content') != null`);
  await typeIn(page, name, sql);
  return name;
}

/** Runs a quick cell and waits until its rows are there and it no longer runs. */
async function runQuick(page, name) {
  await page.eval(click(cell(name), "run"));
  await page.waitFor(`/rows?/.test(${cell(name)}.querySelector('[data-field=status]')?.textContent ?? '') && !${cell(name)}.classList.contains('running')`, 30_000);
}

async function showChart(page, name) {
  await page.eval(`[...${cell(name)}.querySelectorAll('.view-toggle button')].find(b => b.textContent === 'Chart').click()`);
  await page.waitFor(`${cell(name)}.querySelector('.notebook-result svg text.chart-title') !== null`);
}

const svgText = (name, cls) => `(${cell(name)}.querySelector('.notebook-result svg .${cls}')?.textContent ?? null)`;

async function charts(page) {
  // One series: an automatic title and both axis titles, no right axis.
  const one = await addSql(page, "SELECT region, amount FROM sales ORDER BY id");
  await runQuick(page, one);
  await showChart(page, one);
  assert.equal(await page.eval(svgText(one, "chart-title")), "amount by region");
  assert.equal(await page.eval(svgText(one, "chart-x-title")), "region");
  assert.equal(await page.eval(svgText(one, "chart-y-title")), "amount");
  assert.equal(await page.eval(`${cell(one)}.querySelector('.notebook-result svg .chart-y2-title')`), null, "one series: one axis");
  assert.equal(await page.eval(`${cell(one)}.querySelector('.notebook-result svg .chart-axis-right')`), null);

  // Two series of the same scale share the axis, which is then just "Value".
  const same = await addSql(page, "SELECT region, amount, amount / 2 AS half FROM sales ORDER BY id");
  await runQuick(page, same);
  await showChart(page, same);
  assert.equal(await page.eval(svgText(same, "chart-y-title")), "Value");
  assert.equal(await page.eval(`${cell(same)}.querySelector('.notebook-result svg .chart-y2-title')`), null);

  // Two series 10x or more apart: the second on a right axis, named in the legend.
  const two = await addSql(page, "SELECT region, amount, orders FROM sales ORDER BY id");
  await runQuick(page, two);
  await showChart(page, two);
  assert.equal(await page.eval(svgText(two, "chart-title")), "amount, orders by region");
  assert.equal(await page.eval(svgText(two, "chart-y-title")), "amount");
  assert.equal(await page.eval(svgText(two, "chart-y2-title")), "orders");
  assert.equal(await page.eval(`${cell(two)}.querySelectorAll('.notebook-result svg .chart-axis-right text').length > 1`), true, "ticks of its own");
  assert.match(await page.eval(`${cell(two)}.querySelector('.notebook-result svg .chart-legend').textContent`), /orders \(right axis\)/);
  // The small series is not flat: its tallest bar is a good part of the plot.
  const heights = await page.eval(
    `[...${cell(two)}.querySelectorAll('.notebook-result svg .chart-bar')].filter(b => b.querySelector('title').textContent.includes('orders:')).map(b => +b.getAttribute('height'))`,
  );
  assert.ok(Math.max(...heights) > 40, `orders bars drawn on their own scale: ${heights}`);
}

async function exportAndSave(page, dir) {
  const html = join(dir, "ux.html");
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(html)})`);
  await page.eval(click(tab, "export_html"));
  await page.waitFor(`${toasts}.includes('Exported to ux.html')`, 15_000);
  assert.match(readFileSync(html, "utf8"), /amount, orders by region/, "the exported chart keeps its title");

  const file = join(dir, "ux.pnb");
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(file)})`);
  await page.eval(click(tab, "save"));
  await page.waitFor(`${toasts}.includes('Notebook saved')`, 15_000);
  assert.ok(existsSync(file));

  // A file that cannot be written says why, in an error toast.
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(join(dir, "missing", "x.html"))})`);
  await page.eval(click(tab, "export_html"));
  await page.waitFor(`document.querySelector('.toast.error') !== null`, 15_000);
}

const running = (name) => `${cell(name)}.classList.contains('running')`;

async function stopping(page) {
  const slow = await addSql(page, SLOW);
  await page.eval(click(cell(slow), "run"));
  await page.waitFor(`${running(slow)} && ${cell(slow)}.querySelector('.notebook-running') !== null && ${cell(slow)}.querySelector('[data-action=stop]') !== null`, 15_000);
  assert.equal(await page.eval(`${cell(slow)}.querySelector('[data-action=run]')`), null, "Run gives way to Stop");
  await page.eval(click(cell(slow), "stop"));
  await page.waitFor(`!${running(slow)}`, 30_000);
  assert.equal(await page.eval(`${cell(slow)}.querySelector('.notebook-running')`), null, "the indicator goes");
  await page.waitFor(`${cell(slow)}.querySelector('[data-action=run]') !== null`);
  // Stopped: it ends with no rows of its own, not with the count.
  assert.doesNotMatch(await page.eval(`${cell(slow)}.querySelector('[data-field=status]')?.textContent ?? ''`), /rows?\b/, "stopped: no full result");

  // Stop during "Run all": the cell after the slow one never starts.
  const after = await addSql(page, "SELECT 1 AS after_slow");
  await page.eval(click(tab, "run_all"));
  await page.waitFor(`${tab}.querySelector('[data-action=stop_all]') !== null`, 15_000);
  await page.waitFor(`${running(slow)} && ${cell(slow)}.querySelector('.notebook-running') !== null`, 15_000);
  await page.eval(click(tab, "stop_all"));
  await page.waitFor(`${tab}.querySelector('[data-action=stop_all]') === null && !${running(slow)}`, 30_000);
  assert.equal(await page.eval(`${cell(after)}.querySelector('[data-field=status]')`), null, "the next cell did not run");
  assert.equal(await page.eval(`${tab}.querySelector('[data-action=run_all]').disabled`), false);
}

export default async function (page, { dir }) {
  const before = await page.eval(`document.querySelectorAll('.notebook-tab').length`);
  await treeMenu(page, NAME, "New notebook");
  await page.waitFor(`document.querySelectorAll('.notebook-tab').length > ${before} && ${cell("c1")} != null`);
  await charts(page);
  await exportAndSave(page, dir);
  await stopping(page);
}
