// BUG ANA-05 (medium, wrong data shown): column statistics compare intervals (and durations) as
// their text, so Minimum and Maximum are wrong: "1 days" < "2 hours" as text, hence a column with
// one day and two hours reports Minimum "1 days" and Maximum "2 hours". Same on PostgreSQL
// `interval` ("-1 days" … "2 hours" with "1 days" in the column).
// Steps: run `SELECT unnest([INTERVAL 1 DAY, INTERVAL 2 HOUR]) AS iv`; rest the mouse on its header.
// Expected: Minimum "2 hours", Maximum "1 days" (printed "02:00:00" and "1 day" since intervals
// read as the databases print them). Actual: the reverse.
// Suspected: crates/platypus-local/src/values.rs `kind` (Duration and Interval are `Kind::Text`);
// stats.rs `text_profile` orders by text.
import assert from "node:assert/strict";
import { duckConnection, newEditor, profileOf, runQuery, status } from "../qa-ana.mjs";

const NAME = "QA ana intervals";

export function setup({ dir }) {
  duckConnection(dir, NAME, "00000000-0000-4000-8000-0000000a0105");
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT unnest([INTERVAL 1 DAY, INTERVAL 2 HOUR]) AS iv");
  assert.equal(await page.eval(status), "2 rows");

  const { fields } = await profileOf(page, "iv");
  assert.equal(fields.Minimum, "02:00:00");
  assert.equal(fields.Maximum, "1 day");
}
