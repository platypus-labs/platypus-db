// DBS-20 (low, cosmetic): intervals are shown, copied and exported in Arrow's debug form on
// every database that has them (DuckDB, PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB):
// INTERVAL '1 day 2 hours 3 minutes 4 seconds' reads "1 days 2 hours 3 mins 4.000000000 secs",
// INTERVAL '1 hour' reads "1 hours", INTERVAL '1 month 1.5 seconds' "1 mons 1.500000000 secs".
// DuckDB's and PostgreSQL's own clients, and DBeaver, show "1 day 02:03:04", "01:00:00",
// "1 mon 00:00:01.5".
//
// Steps: DuckDB file (no server needed); SELECT INTERVAL '1 day 2 hours 3 minutes 4 seconds'
// AS a, INTERVAL '1 hour' AS b; copy the grid.
// Expected: "1 day 02:03:04" and "01:00:00" (the databases' own text form).
// Actual: "1 days 2 hours 3 mins 4.000000000 secs" and "1 hours".
// Suspected: the value formatting of Arrow Interval(MonthDayNano) arrays (arrow_cast's
// display, used as is by apps/desktop/src-tauri/src/display.rs).
import assert from "node:assert/strict";
import { join } from "node:path";
import { conns, openEditor, rows, run, stubClipboard } from "../qa-dbs.mjs";

export function setup({ dir }) {
  conns(dir, [["QA Duck", { duckdb: join(dir, "i.duckdb") }]]);
}

export default async function (page) {
  await openEditor(page, "QA Duck", null);
  await stubClipboard(page);
  await run(page, "SELECT INTERVAL '1 day 2 hours 3 minutes 4 seconds' AS a, INTERVAL '1 hour' AS b");
  assert.deepEqual(await rows(page), [["1 day 02:03:04", "01:00:00"]]);
}
