// SQL-17. PostgreSQL 'infinity' / '-infinity' dates and timestamps are shown as an error text
// in the cell: "ERROR: Cast error: Failed to convert 2147483647 to temporal for Date32", "ERROR:
// Cast error: Failed to convert 9223372036854775807 to datetime for Timestamp(µs)". DuckDB's
// 'infinity'::DATE shows the same. These values are common as "no end" markers in valid_to
// columns.
// Steps: PostgreSQL, Ctrl+Enter on the SELECT below, copy the grid with headers.
// Expected: infinity, infinity, -infinity (as psql prints them).
// Actual: "ERROR: Cast error ..." in each cell.
// Code: the PG driver maps date/timestamp to Arrow Date32/Timestamp keeping PG's sentinel
// values; the grid's display cast fails on them.
import assert from "node:assert/strict";
import { DB, gridTsv, openEditor, runStatement, saveConnections, skipUnless } from "../editor-helpers.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

export default async function (page) {
  await openEditor(page, DB.pg.name);
  await runStatement(page, "SELECT 'infinity'::date AS d, 'infinity'::timestamp AS ts, '-infinity'::timestamptz AS tz");
  const tsv = await gridTsv(page);
  assert.doesNotMatch(tsv, /ERROR/, tsv);
  assert.equal(tsv, "d\tts\ttz\ninfinity\tinfinity\t-infinity");
}
