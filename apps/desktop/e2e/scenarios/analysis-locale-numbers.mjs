// BUG ANA-15 (low): the column statistics and the chart format numbers with the operating
// system's locale, not the interface language: with the interface in English on an Italian
// Windows the box says "NULL values 33,3%" and the chart's axis "0,8", tooltips "18,19K", while the
// rest of the English UI (grid, status) uses a decimal point. (Passes on an English system.)
// Steps: interface in English; run a query with one NULL in three rows; rest the mouse on the header.
// Expected: "33.3%". Actual on an Italian system: "33,3%".
// Suspected: apps/desktop/src/analysis/ColumnProfile.tsx (`new Intl.NumberFormat(undefined, …)`),
// chartScale.ts `formatNumber(v, locale?)` called without the UI language.
import assert from "node:assert/strict";
import { liteConnection, newEditor, profileOf, runQuery, status } from "../analysis-helpers.mjs";

const NAME = "QA ana locale";

export function setup({ dir }) {
  liteConnection(dir, NAME, "00000000-0000-4000-8000-0000000a0115", "CREATE TABLE t (id INTEGER PRIMARY KEY);");
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT column1 AS v FROM (VALUES (1), (NULL), (2))");
  assert.equal(await page.eval(status), "3 rows");
  const { fields } = await profileOf(page, "v");
  assert.equal(fields["NULL values"], "33.3%");
}
