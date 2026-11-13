// BUG ANA-13 (low): the bounds under a column's histogram are computed in f64 and show numbers
// that are not in the data: a BIGINT column whose largest value is 9223372036854775807 ends its
// range at "9223372036854775808" (past the type's maximum), a numeric(30,10) at
// 12345678901234567890.1234567890 ends at "12345678901234567168", and 1e30 is written with 31
// digits ("1000000015047466219876688855040" for a float4). Minimum and Maximum above are right.
// Steps: run `SELECT unnest([0, 9223372036854775807]::BIGINT[]) AS v`; rest the mouse on its header.
// Expected: the range under the bars ends at the Maximum shown above it. Actual: 9223372036854775808.
// Suspected: crates/platypus-local/src/stats.rs `ranges` (labels from `values::format_number` of
// the f64 bounds; the first and last bound could be the shown extremes).
import assert from "node:assert/strict";
import { duckConnection, newEditor, profileOf, runQuery, status } from "../analysis-helpers.mjs";

const NAME = "QA ana histogram";

export function setup({ dir }) {
  duckConnection(dir, NAME, "00000000-0000-4000-8000-0000000a0106");
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT unnest([0, 9223372036854775807]::BIGINT[]) AS v");
  assert.equal(await page.eval(status), "2 rows");

  const { fields, range } = await profileOf(page, "v");
  assert.equal(fields.Maximum, "9223372036854775807");
  assert.deepEqual(range, [fields.Minimum, fields.Maximum], "the histogram's ends are the extremes");
}
