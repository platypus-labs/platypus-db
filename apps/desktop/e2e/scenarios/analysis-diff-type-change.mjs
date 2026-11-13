// BUG ANA-11 (medium): "Compare with previous run" is offered (enabled) but fails with "the two
// results do not have the same columns" when a column keeps its name and declared type but its
// Arrow type changes between runs. On SQLite a NUMERIC column read as integers becomes a float
// column once one value is 2.5; on PostgreSQL/MySQL the same happens for expressions whose type
// is told from the values. The button decides on name + database type (diffModel.ts
// `sameColumns`), the backend on the Arrow type (platypus-local diff.rs `same_columns`). The error
// is also the raw lower-case backend text, not a sentence of the UI.
// Steps: table prices(id INTEGER PRIMARY KEY, price NUMERIC) with 10 and 20; run SELECT * FROM
// prices; set price = 2.5 for id 2 elsewhere; run again; click Compare with previous run.
// Expected: the diff tab, one changed row (20 → 2.5) — or, if not comparable, the button disabled.
// Actual: an error toast, no diff.
// Suspected: crates/platypus-local/src/diff.rs `same_columns` (could compare after casting numbers
// to a common type, or compare the shown text), apps/desktop/src/analysis/diffModel.ts.
import assert from "node:assert/strict";
import { compareButton, diffTab, liteConnection, newEditor, rerun, runQuery, sqliteExec, status } from "../analysis-helpers.mjs";

const NAME = "QA ana diff types";
let file;

export function setup({ dir }) {
  file = liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000a0112",
    "CREATE TABLE prices (id INTEGER PRIMARY KEY, price NUMERIC); INSERT INTO prices VALUES (1, 10), (2, 20);",
  );
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT * FROM prices ORDER BY id");
  assert.equal(await page.eval(status), "2 rows");
  sqliteExec(file, "UPDATE prices SET price = 2.5 WHERE id = 2");
  await rerun(page);
  assert.equal(await page.eval(status), "2 rows");

  await page.waitFor(`${compareButton} != null`);
  if (await page.eval(`${compareButton}.disabled`)) return; // Not comparable, and said so: fine.
  await page.eval(`${compareButton}.click()`);
  await page.waitFor(
    `${diffTab}?.querySelector('.diff-counts') != null || [...document.querySelectorAll('.toast, .notice.error, .error')].some(e => /same columns/i.test(e.textContent))`,
    10_000,
  );
  assert.equal(await page.eval(`${diffTab}?.querySelector('.diff-counts')?.textContent ?? null`), "+0 −0 ~1", "the diff opens");
}
