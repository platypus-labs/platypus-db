// JRN-12 (low). Counts in the interface's sentences are grouped with the operating system's
// locale, not the interface language: with the interface in English on an Italian Windows
// the status says "100.000 rows" and the comparison "Compared the first 100.000 and 1500
// loaded rows" (both separators in one sentence); with the interface in Italian on an
// English Windows it would say "1,500 righe". (Same family as ANA-15, which is about the
// statistics and the chart; this one is every `plural()` count.) Passes on an English system.
// Steps: interface in English; a query of 1500 rows; Fetch all.
// Expected: "1,500 rows". Actual on an Italian system: "1500 rows" (Italian groups from five
// digits: 100000 rows show as "100.000 rows").
// Suspected: apps/desktop/src/i18n.ts:57 `plural` (`n.toLocaleString()` without the
// interface language).
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, status, treeRow, visibleTab } from "../query.mjs";

const NAME = "QA jrn count";

export function setup({ dir }) {
  const values = Array.from({ length: 1500 }, (_, i) => `(${i + 1})`).join(",");
  sqliteConnection(dir, {
    file: "qa_jrn_count.db",
    id: "00000000-0000-4000-8000-0000000a8005",
    name: NAME,
    sql: `CREATE TABLE n (id INTEGER PRIMARY KEY); INSERT INTO n VALUES ${values};`,
  });
}

export default async function (page) {
  await page.eval(`${treeRow(NAME)}.id = 'jrn-c'`);
  await page.clickSel("#jrn-c", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("SELECT id FROM n");
  await page.key("Escape", "Escape", 27);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.statusbar button')].some(b => b.textContent === 'Fetch all')`, 15_000);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`[...${visibleTab}.querySelectorAll('.statusbar button')].find(b => b.textContent === 'Fetch all').click()`);
  await page.waitFor(`/^[\\d.,]+ rows$/.test(${status})`, 15_000);
  assert.equal(await page.eval(status), "1,500 rows");
}
