// BUG ANA-02 (critical, wrong data shown): SQL on results turns a NULL INTERVAL into "0 secs".
// The copy of the rows in the in-memory DuckDB loses the NULLs of interval columns: the SQL view
// shows "0 secs" where the result shows NULL, `iv IS NULL` is false and any count, filter or
// aggregate over the column is wrong. Seen with PostgreSQL `interval` and DuckDB `INTERVAL`.
// Steps: run a query with an INTERVAL column holding a NULL; in the field above the grid run
// `SELECT count(*) FILTER (WHERE iv IS NULL) AS nulls, … FROM result`.
// Expected: 1 NULL, the SQL view shows NULL. Actual: 0 NULLs, the cell shows "0 secs".
// Suspected: crates/platypus-local/src/duck.rs `load` (Arrow IntervalMonthDayNano through the
// `platypus_arrow` scan of duckdb-rs, whose interval conversion ignores the validity bitmap).
import assert from "node:assert/strict";
import { duckConnection, gridTsv, inField, newEditor, runQuery, status } from "../qa-ana.mjs";

const NAME = "QA ana interval";

export function setup({ dir }) {
  duckConnection(dir, NAME, "00000000-0000-4000-8000-0000000a0102");
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT * FROM (VALUES (1, INTERVAL 1 DAY), (2, NULL)) t(id, iv) ORDER BY id");
  assert.equal(await page.eval(status), "2 rows");
  assert.equal(await gridTsv(page), "id\tiv\n1\t1 day\n2\t", "the result itself shows the NULL");

  const run = await inField(page, "SELECT count(*) FILTER (WHERE iv IS NULL) AS nulls, count(iv) AS filled FROM result");
  assert.equal(run.error, "");
  assert.equal(await gridTsv(page), "nulls\tfilled\n1\t1", "the copy keeps the NULL interval");

  await inField(page, "SELECT * FROM result ORDER BY id");
  assert.equal(await gridTsv(page), "id\tiv\n1\t1 day\n2\t", "the SQL view shows NULL, not 0 secs");
}
