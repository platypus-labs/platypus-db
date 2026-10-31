// BUG qa4-ana-1 (medium, wrong data shown): a chart of small numbers writes every axis label and
// every tooltip as "0".
// Steps: run `SELECT 'a' AS k, 0.000001 AS v UNION ALL SELECT 'b', 0.0000035 UNION ALL SELECT 'c', 0.000002`;
// switch the result to Chart (bars, sum of v by k).
// Expected: axis labels that tell the values apart (1e-6 … 3.5e-6) and tooltips with the value.
// Actual: all five axis labels read "0" and the tooltips "a · v: 0", "b · v: 0", "c · v: 0".
// Code: apps/desktop/src/analysis/chartScale.ts `formatNumber` (compact notation with
// maximumFractionDigits: 2 rounds anything under 0.005 to 0).
import assert from "node:assert/strict";
import { chart, liteConnection, markTitles, newEditor, runQuery, showChart, status } from "../qa-ana.mjs";

const NAME = "QA4 ana small values";

export function setup({ dir }) {
  liteConnection(dir, NAME, "00000000-0000-4000-8000-0000000b0101", "CREATE TABLE t (id INTEGER PRIMARY KEY);");
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT 'a' AS k, 0.000001 AS v UNION ALL SELECT 'b', 0.0000035 UNION ALL SELECT 'c', 0.000002");
  assert.equal(await page.eval(status), "3 rows");
  await showChart(page, "rect.chart-bar");
  const titles = await markTitles(page, "rect.chart-bar");
  assert.ok(!titles.some((t) => /: 0$/.test(t)), `tooltips keep the value: ${JSON.stringify(titles)}`);
  const ticks = await page.eval(`[...${chart}.querySelectorAll('.chart-axis text')].map(t => t.textContent)`);
  assert.ok(new Set(ticks).size > 2, `axis labels differ: ${JSON.stringify(ticks)}`);
}
