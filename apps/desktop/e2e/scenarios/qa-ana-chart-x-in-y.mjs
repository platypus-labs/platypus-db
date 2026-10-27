// BUG ANA-08 (medium): choosing as X axis a column that is one of the Y series keeps it as a series:
// it is drawn against itself (a diagonal of points) and stays in the legend, while its Y checkbox is
// hidden, so it can no longer be unchecked.
// Steps: run `SELECT region, amount, qty …`; Chart (X region, Y amount and qty); choose X = amount.
// Expected: the series are the Y columns other than X (qty only), legend without "amount".
// Actual: legend "amount", "qty"; the amount series plots amount over amount.
// Suspected: apps/desktop/src/analysis/ChartView.tsx, the X select's onChange (`set({ x })` keeps
// `y`); chartConfig.ts `toRequest` could also drop X from Y.
import assert from "node:assert/strict";
import { chart, chooseAndRedraw, liteConnection, newEditor, runQuery, showChart, status, visibleTab, yBox } from "../qa-ana.mjs";

const NAME = "QA ana x in y";

export function setup({ dir }) {
  liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000a0110",
    `CREATE TABLE sales (id INTEGER PRIMARY KEY, region TEXT, amount REAL, qty INTEGER);
     INSERT INTO sales VALUES (1, 'n', 10, 1), (2, 's', 5, 2), (3, 'n', 7, 3), (4, 'e', 3, 4);`,
  );
}

const legend = `[...${chart}.querySelectorAll('.chart-legend text')].map(t => t.textContent)`;

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT region, amount, qty FROM sales ORDER BY id");
  assert.equal(await page.eval(status), "4 rows");
  await showChart(page, "rect.chart-bar");
  assert.deepEqual(await page.eval(legend), ["amount", "qty"]);

  await chooseAndRedraw(page, "X axis", "amount");
  assert.equal(await page.eval(`${yBox("amount")} ?? null`), null, "X has no Y checkbox");
  const series = await page.eval(legend);
  assert.ok(!series.includes("amount"), `the X column is not a series: ${JSON.stringify(series)}`);
  assert.equal(await page.eval(`${visibleTab}.querySelector('select[aria-label="X axis"]').value`), "amount");
}
