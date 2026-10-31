// BUG qa4-ana-2 (low, visual): in a table profile at the default window size the card's figures are
// cut with an ellipsis: "Empty strings 257 (12.8…", "Length 0 · 4.805…", "Quartiles 501 · 1,001 ·…"
// (the third quartile is lost), and the same for the min/max of long texts. The value column is
// only ~120px wide while the histogram column beside it has room to spare.
// Steps: open Profile data… of a table of 2,000 rows with a numeric column (1..2000) and a text
// column with 12.8% empty strings, at the default 1280px window.
// Expected: every figure of the first block is shown whole (scrollWidth <= clientWidth).
// Actual: Quartiles and the percentages overflow and show "…".
// Code: apps/desktop/src/analysis/tableProfile.css `.tp-card` grid (`180px 220px minmax(120px, 1fr)
// minmax(160px, 240px)`): the dl in the 220px column minus its label column leaves ~120px.
import assert from "node:assert/strict";
import { join } from "node:path";
import { liteConnection, treeRow } from "../qa-ana.mjs";

const NAME = "QA4 ana profile cut";
const tab = `[...document.querySelectorAll('.table-profile-tab')].find(t => t.style.display !== 'none')`;

export function setup({ dir }) {
  liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000b0102",
    `CREATE TABLE prof (id INTEGER PRIMARY KEY, name TEXT);
     WITH RECURSIVE g(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM g WHERE n < 2000)
       INSERT INTO prof SELECT n, CASE WHEN n % 8 = 0 THEN '' ELSE 'name' || (n % 25) END FROM g;`,
  );
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("prof")} !== undefined`, 15_000);
  await page.eval(`${treeRow("prof")}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === 'Profile data…').click()`);
  await page.waitFor(`${tab}?.querySelector('.tp-status')?.textContent.includes('2,000 rows read')`, 30_000);
  await page.sleep(500);
  const cut = await page.eval(`[...${tab}.querySelectorAll('.tp-card dd, .tp-card dd *')]
    .filter(e => e.scrollWidth > e.clientWidth + 1)
    .map(e => e.closest('dd')?.previousElementSibling?.textContent + ': ' + e.textContent)`);
  assert.deepEqual([...new Set(cut)], [], "figures are shown whole");
}
