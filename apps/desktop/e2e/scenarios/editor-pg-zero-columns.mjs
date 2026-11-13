// SQL-29 (low). PostgreSQL queries without columns (`SELECT FROM t`, `SELECT`, used to count
// or test existence) fail with an internal message: "Invalid argument error: must either
// specify a row count or at least one column" (an Arrow error, not the server's).
// Steps: PostgreSQL, Ctrl+Enter on `SELECT FROM generate_series(1, 3)`.
// Expected: "3 rows" with no columns (psql prints "(3 rows)").
// Actual: Error with the Arrow message.
// Code: crates/platypus-driver-pg builds a RecordBatch with no columns and no row count
// (RecordBatchOptions::with_row_count is needed).
import assert from "node:assert/strict";
import { DB, openEditor, resultError, runStatement, saveConnections, skipUnless } from "../editor-helpers.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

export default async function (page) {
  await openEditor(page, DB.pg.name);
  const status = await runStatement(page, "SELECT FROM generate_series(1, 3)");
  assert.equal(await page.eval(resultError), null);
  assert.equal(status, "3 rows");
}
