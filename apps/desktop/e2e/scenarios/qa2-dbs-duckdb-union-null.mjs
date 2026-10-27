// DBS-03 (high, wrong value shown): a NULL of a DuckDB UNION column is shown, copied and
// exported as "{n=}" instead of NULL, so it cannot be told apart from a value; a non-NULL
// value shows as "{n=5}" where DuckDB itself shows 5.
//
// Steps: DuckDB file; CREATE TABLE u (id INT, un UNION(n INT, s VARCHAR)); INSERT (1, 5),
// (2, NULL); SELECT * FROM u ORDER BY id; copy the grid (Ctrl+A, Ctrl+C).
// Expected: row 2's un is NULL (empty in the copy, "NULL" in the grid, like the other columns).
// Actual: "{n=}" (and row 1 "{n=5}").
// Suspected: the display of Arrow union arrays (the grid's / transfer's value formatting of
// DataType::Union), which ignores the union's null (a null child of the selected type).
import assert from "node:assert/strict";
import { join } from "node:path";
import { conns, openEditor, rows, run, stubClipboard } from "../qa-dbs.mjs";

export function setup({ dir }) {
  conns(dir, [["QA Duck", { duckdb: join(dir, "u.duckdb") }]]);
}

export default async function (page) {
  await openEditor(page, "QA Duck", null);
  await stubClipboard(page);
  await run(page, "CREATE TABLE u (id INT, un UNION(n INT, s VARCHAR)); INSERT INTO u VALUES (1, 5), (2, NULL);", true);
  await run(page, "SELECT id, un FROM u ORDER BY id");
  const got = await rows(page);
  assert.deepEqual(got[1], ["2", ""], `a NULL union is NULL: ${JSON.stringify(got)}`);
  assert.deepEqual(got[0], ["1", "5"]);
}
