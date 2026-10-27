// BUG ANA-16 (low): a pie leaves out the categories whose value is negative or zero but still lists
// them in the legend, with a colour that matches no slice; nothing says they are left out. With
// several Y columns checked, a pie silently draws only the first one.
// Steps: run a query with categories a (5), b (-3), c (2); Chart, type Pie.
// Expected: the legend lists only the slices drawn, or a note says that b (negative) is left out.
// Actual: legend a, b, c over two slices.
// Suspected: apps/desktop/src/analysis/Chart.tsx `Chart` (legend = all categories for a pie) and
// `Pie`; chartScale.ts `pieAngles` counts negatives as 0.
import assert from "node:assert/strict";
import { chart, chooseAndRedraw, liteConnection, newEditor, runQuery, showChart, status, visibleTab } from "../qa-ana.mjs";

const NAME = "QA ana pie";

export function setup({ dir }) {
  liteConnection(dir, NAME, "00000000-0000-4000-8000-0000000a0114", "CREATE TABLE t (id INTEGER PRIMARY KEY);");
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT column1 AS k, column2 AS n FROM (VALUES ('a', 5), ('b', -3), ('c', 2))");
  assert.equal(await page.eval(status), "3 rows");
  await showChart(page, "rect.chart-bar");
  await chooseAndRedraw(page, "Chart type", "pie");
  await page.waitFor(`${chart}?.querySelectorAll('path.chart-slice').length > 0`);
  const slices = await page.eval(`${chart}.querySelectorAll('path.chart-slice').length`);
  const legend = await page.eval(`[...${chart}.querySelectorAll('.chart-legend text')].map(t => t.textContent)`);
  const note = await page.eval(`${visibleTab}.querySelector('.chart-main').textContent`);
  assert.equal(slices, 2);
  assert.ok(legend.length === slices || /negative|left out/i.test(note), `legend ${JSON.stringify(legend)} over ${slices} slices, no note`);
}
