// QA4 grid2 DuckDB (medium: information lost in the grid). A TIMETZ value is shown without its
// UTC offset: TIMETZ '03:04:05+02' reads "03:04:05" in the grid, while DuckDB itself prints
// 03:04:05+02 (the same value cast to VARCHAR keeps the offset, as the second column shows), so
// two different instants of the day look equal and the value cannot be edited back faithfully.
// Steps: DuckDB editor, SELECT TIMETZ '03:04:05+02' AS t, TIMETZ '03:04:05+02'::VARCHAR AS s; copy.
// Expected: 03:04:05+02 in both columns. Actual: 03:04:05 and 03:04:05+02.
// Code (likely): the DuckDB driver reads the result through DuckDB's Arrow export, which turns TIMETZ
// into a plain Arrow time of day and drops the offset.
import { join } from "node:path";
import { assert, connections, copyAll, openEditor, run, stubClipboard } from "../grid-helpers.mjs";

export function setup({ dir }) {
  connections(dir, [["QA DUCK", { duckdb: join(dir, "q.duckdb") }]]);
}

export default async function (page) {
  await openEditor(page, "QA DUCK", null, null);
  await stubClipboard(page);
  await run(page, "SELECT TIMETZ '03:04:05+02' AS t, TIMETZ '03:04:05+02'::VARCHAR AS s");
  assert.equal(await copyAll(page), "03:04:05+02\t03:04:05+02");
}

