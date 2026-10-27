// BUG ANA-12 (high): SQL on results fails for a whole result when a HUGEINT (or a number with
// more than 38 digits) is in it. The result shows the value; the field answers "DuckDB: Invalid
// Input Error: invalid Arrow decimal value at row 0: decimal value has 39 digits, exceeding
// width 38" whatever the query, even one that does not read that column.
// Steps: on DuckDB run `SELECT 170141183460469231731687303715884105727::HUGEINT AS h, 1 AS n`;
// in the field above the grid run `SELECT n FROM result`.
// Expected: 1 row. Actual: the error above, nothing can be queried.
// Suspected: crates/platypus-local/src/duck.rs `takes`/`storable`: a Decimal128 whose values
// overflow its declared precision (the driver's HUGEINT as Decimal128(38, 0)) should be stored as
// text or as a wider type instead of being bound as is.
import assert from "node:assert/strict";
import { duckConnection, gridTsv, inField, newEditor, runQuery, status } from "../qa-ana.mjs";

const NAME = "QA ana hugeint";

export function setup({ dir }) {
  duckConnection(dir, NAME, "00000000-0000-4000-8000-0000000a0103");
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT 170141183460469231731687303715884105727::HUGEINT AS h, 1 AS n");
  assert.equal(await page.eval(status), "1 row");

  const run = await inField(page, "SELECT n FROM result");
  assert.equal(run.error, "", "the copy of the result loads");
  assert.equal(await gridTsv(page), "n\n1");
}
