// BUG ANA-03 (high, wrong data shown): column statistics pick Minimum and Maximum through f64, so
// integers beyond 2^53 that round to the same double come out wrong, even with Minimum > Maximum:
// 9007199254740993 and 9007199254740992 give Minimum 9007199254740993, Maximum 9007199254740992.
// Same for BIGINT (PostgreSQL, SQLite), UBIGINT (DuckDB: max 18446744073709551614 shown while
// 18446744073709551615 is in the column), BIGINT UNSIGNED (MySQL), HUGEINT and wide decimals.
// Steps: run `SELECT 9007199254740993 AS v UNION ALL SELECT 9007199254740992`; rest the mouse on
// the header of v.
// Expected: Minimum 9007199254740992, Maximum 9007199254740993. Actual: swapped.
// Suspected: crates/platypus-local/src/stats.rs `ranges` (extremes found on `values::numbers`,
// f64; ties keep the first minimum and the last maximum).
import assert from "node:assert/strict";
import { liteConnection, newEditor, profileOf, runQuery, status } from "../qa-ana.mjs";

const NAME = "QA ana bigint";

export function setup({ dir }) {
  liteConnection(dir, NAME, "00000000-0000-4000-8000-0000000a0107", "CREATE TABLE t (id INTEGER PRIMARY KEY);");
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT 9007199254740993 AS v UNION ALL SELECT 9007199254740992");
  assert.equal(await page.eval(status), "2 rows");

  const { fields } = await profileOf(page, "v");
  assert.equal(fields.Minimum, "9007199254740992");
  assert.equal(fields.Maximum, "9007199254740993");
}
