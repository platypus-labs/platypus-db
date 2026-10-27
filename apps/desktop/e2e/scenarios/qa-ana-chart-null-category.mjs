// BUG ANA-06 (medium, wrong data shown): bars and pie put SQL NULL and the text 'NULL' in one
// category: their values add up under a single "NULL" bar. Lines and scatter over a text X do the
// same.
// Steps: run `SELECT * FROM (VALUES (NULL, 1), ('NULL', 4), ('a', 2)) …` with columns k, n; switch
// the result to Chart (bars, sum of n by k).
// Expected: three bars (a NULL category for the NULLs, 1, and one for the text 'NULL', 4).
// Actual: two bars, "NULL · n: 5".
// Suspected: crates/platypus-local/src/chart.rs `by_category` and `x_values` (NULL is keyed by the
// text "NULL" in the same map as the values' texts).
import assert from "node:assert/strict";
import { liteConnection, markTitles, newEditor, runQuery, showChart, status } from "../qa-ana.mjs";

const NAME = "QA ana null category";

export function setup({ dir }) {
  liteConnection(dir, NAME, "00000000-0000-4000-8000-0000000a0108", "CREATE TABLE t (id INTEGER PRIMARY KEY);");
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT column1 AS k, column2 AS n FROM (VALUES (NULL, 1), ('NULL', 4), ('a', 2))");
  assert.equal(await page.eval(status), "3 rows");

  await showChart(page, "rect.chart-bar");
  const bars = await markTitles(page, "rect.chart-bar");
  assert.equal(bars.length, 3, `one bar per category: ${JSON.stringify(bars)}`);
  assert.ok(!bars.includes("NULL · n: 5"), "NULL and 'NULL' are not added up");
}
