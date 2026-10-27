// BUG ANA-07 (medium, wrong data shown): with Average, Minimum or Maximum, a category whose Y
// values are all NULL is drawn as a bar of 0 (tooltip "a · n: 0"), as if its average were 0.
// SQL's avg/min/max are NULL there; the chart should leave the bar out (or mark it), not invent 0.
// Steps: run a query with k = 'a' twice with n NULL and k = 'b' with n = 3; Chart, bars, Average.
// Expected: no bar of value 0 for "a". Actual: "a · n: 0".
// Suspected: crates/platypus-local/src/chart.rs `Acc::value` (`_ if self.count == 0 => 0.0`).
import assert from "node:assert/strict";
import { chooseAndRedraw, liteConnection, markTitles, newEditor, runQuery, showChart, status } from "../qa-ana.mjs";

const NAME = "QA ana null average";

export function setup({ dir }) {
  liteConnection(dir, NAME, "00000000-0000-4000-8000-0000000a0109", "CREATE TABLE t (id INTEGER PRIMARY KEY);");
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT column1 AS k, column2 AS n FROM (VALUES ('a', NULL), ('a', NULL), ('b', 3))");
  assert.equal(await page.eval(status), "3 rows");

  await showChart(page, "rect.chart-bar");
  for (const aggregate of ["avg", "min", "max"]) {
    await chooseAndRedraw(page, "Aggregate", aggregate);
    const bars = await markTitles(page, "rect.chart-bar");
    assert.ok(bars.includes("b · n: 3"), `${aggregate}: ${JSON.stringify(bars)}`);
    assert.ok(!bars.includes("a · n: 0"), `${aggregate} of only NULLs is not 0: ${JSON.stringify(bars)}`);
  }
}
