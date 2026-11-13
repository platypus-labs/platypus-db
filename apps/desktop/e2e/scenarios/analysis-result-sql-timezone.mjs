// BUG qa4-ana-4 (medium, wrong data shown): SQL on a result reads timestamptz columns in the
// computer's time zone, while the grid shows them as the database sent them. Grouping by day or
// extracting the hour from the result gives other values than the ones on screen.
// Steps: PostgreSQL: run `SELECT TIMESTAMPTZ '2024-03-31 00:30:00+00' + g * interval '1 hour' AS ts`
// for g = 1..3; in the result's SQL field run `SELECT ts, strftime(ts, '%H:%M') AS h FROM result`.
// Expected: h equals the time shown in ts (01:30, 02:30, 03:30).
// Actual: on a machine in Europe/Rome h is 03:30, 04:30, 05:30 (DuckDB's session TimeZone is the OS's:
// the loaded rows' timestamptz are instants, `strftime`, `date_trunc('day', …)` and `hour()` use it).
// Code: crates/platypus-local/src/duck.rs (no `SET TimeZone` when the in-memory DuckDB opens).
// The scenario compares with the displayed time, so it passes on a UTC machine.
import assert from "node:assert/strict";
import { gridTsv, inField, newEditor, runQuery, saveConnections } from "../analysis-helpers.mjs";

const NAME = "QA4 ana tz";

export function setup({ dir }) {
  saveConnections(dir, [
    { id: "00000000-0000-4000-8000-0000000b0104", name: NAME, driver: "postgres", host: "localhost", port: 55432, database: "platypus", user: "platypus", password: "platypus" },
  ]);
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT TIMESTAMPTZ '2024-03-31 00:30:00+00' + g * interval '1 hour' AS ts FROM generate_series(1, 3) g");
  const shown = (await gridTsv(page)).split("\n").slice(1).map((v) => v.slice(11, 16));
  assert.deepEqual(shown.length, 3);
  const r = await inField(page, "SELECT ts, strftime(ts, '%H:%M') AS h FROM result");
  assert.equal(r.error, "");
  const rows = (await gridTsv(page)).split("\n").slice(1).map((l) => l.split("\t"));
  assert.deepEqual(
    rows.map((c) => c[1]),
    shown,
    `the hour read by SQL is the hour shown: ${JSON.stringify(rows)}`,
  );
}
