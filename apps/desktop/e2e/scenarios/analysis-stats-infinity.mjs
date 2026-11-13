// BUG ANA-04 (high, wrong data shown): column statistics leave out infinite values. A float
// column holding inf and -inf reports the largest and smallest *finite* values as Maximum and
// Minimum; the histogram also drops them. PostgreSQL shows the same with 'Infinity'.
// Steps: run `SELECT unnest([1.5, 'inf', '-inf', 2.5]::DOUBLE[]) AS v`; rest the mouse on the
// header of v.
// Expected: Minimum -inf, Maximum inf (as the grid shows them, and as min()/max() answer).
// Actual: Minimum 1.5, Maximum 2.5.
// Suspected: crates/platypus-local/src/stats.rs `ranges` (`filter(|v| v.is_finite())` decides the
// extremes too, not only the histogram).
import assert from "node:assert/strict";
import { duckConnection, gridTsv, newEditor, profileOf, runQuery, status } from "../analysis-helpers.mjs";

const NAME = "QA ana infinity";

export function setup({ dir }) {
  duckConnection(dir, NAME, "00000000-0000-4000-8000-0000000a0104");
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT unnest(['1.5', 'inf', '-inf', '2.5']::DOUBLE[]) AS v");
  assert.equal(await page.eval(status), "4 rows");
  assert.equal(await gridTsv(page), "v\n1.5\ninf\n-inf\n2.5");

  const { fields } = await profileOf(page, "v");
  assert.equal(fields.Minimum, "-inf");
  assert.equal(fields.Maximum, "inf");
}
