// ANA-14 (fixed): the chart of a result with no rows drew empty axes from 0 to 1 (ticks 0, 0.2,
// … 1) with no word that there is nothing to draw; the same for a Y column with only NULLs or
// only text. It now says so in place of the chart, as DBeaver and most tools do.
// Steps: run `SELECT region, amount FROM sales WHERE 0`; switch to Chart.
// Expected: "No rows to draw." and no axes.
// Code: apps/desktop/src/analysis/ChartView.tsx (`chart-empty`).
import assert from "node:assert/strict";
import { chart, liteConnection, newEditor, runQuery, status, toggle, visibleTab } from "../qa-ana.mjs";

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
  // No chart is drawn: the notice takes its place.
  await page.eval(`${toggle("Chart")}.click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.chart-empty') != null`, 10_000);
  const main = await page.eval(`${visibleTab}.querySelector('.chart-main').textContent`);
  assert.match(main, /no (rows|data)|nothing to (draw|show)/i, `an empty chart says so: ${main}`);
  assert.equal(await page.eval(`${chart}?.querySelectorAll('.chart-axis text').length ?? 0`), 0, "no axis of made-up ticks");
}
