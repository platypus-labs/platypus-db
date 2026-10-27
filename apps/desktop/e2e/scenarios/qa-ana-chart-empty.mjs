// BUG ANA-14 (low): the chart of a result with no rows draws empty axes from 0 to 1 (ticks 0, 0.2,
// … 1) with no word that there is nothing to draw; the same for a Y column with only NULLs or
// only text. DBeaver and most tools say "No data".
// Steps: run `SELECT region, amount FROM sales WHERE 0`; switch to Chart.
// Expected: a message such as "No rows to draw" in place of the axes. Actual: bare axes.
// Suspected: apps/desktop/src/analysis/ChartView.tsx (no case for `data.rows === 0` or no series
// values); Chart.tsx draws the axes anyway.
import assert from "node:assert/strict";
import { chart, liteConnection, newEditor, runQuery, showChart, status, visibleTab } from "../qa-ana.mjs";

const NAME = "QA ana empty chart";

export function setup({ dir }) {
  liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000a0113",
    "CREATE TABLE sales (id INTEGER PRIMARY KEY, region TEXT, amount REAL); INSERT INTO sales VALUES (1, 'n', 1);",
  );
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT region, amount FROM sales WHERE 0");
  assert.equal(await page.eval(status), "0 rows");
  await showChart(page);
  await page.sleep(500);
  const main = await page.eval(`${visibleTab}.querySelector('.chart-main').textContent`);
  assert.match(main, /no (rows|data)|nothing to (draw|show)/i, `an empty chart says so: ${main}`);
  assert.equal(await page.eval(`${chart}?.querySelectorAll('.chart-axis text').length ?? 0`), 0, "no axis of made-up ticks");
}
