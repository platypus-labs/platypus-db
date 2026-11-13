// BUG qa4-ana-3 (medium, wrong data shown): a pivot puts SQL NULL and the text 'NULL' of the column
// field in one column and adds their cells up (the same mistake the charts had, analysis-chart-null-category).
// Steps: run `SELECT region, m, amount FROM s` over rows (n,'NULL',1), (n,NULL,10), (s,'',100);
// Pivot…: row field region, column field m, value amount, sum.
// Expected: separate columns for the text 'NULL' (1) and for NULL (10).
// Actual: one column "NULL" with 11 in row n.
// Code: apps/desktop/src/analysis/pivot.ts `label` (`coalesce(CAST(col AS VARCHAR), 'NULL')`: the
// label of a NULL is the same text as a value spelt NULL).
import assert from "node:assert/strict";
import { gridTsv, liteConnection, newEditor, runQuery, status, visibleTab } from "../analysis-helpers.mjs";

const NAME = "QA4 ana pivot null";
const dialog = `document.querySelector('.pivot-dialog')`;
const rowBox = (column) => `[...${dialog}.querySelectorAll('fieldset label')].find(l => l.textContent === ${JSON.stringify(column)})?.querySelector('input')`;
const select = (label) => `${dialog}.querySelector('select[aria-label=${JSON.stringify(label)}]')`;
const choose = (label, value) => `(() => {
  const el = ${select(label)};
  Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('change', { bubbles: true }));
})()`;

export function setup({ dir }) {
  liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000b0103",
    `CREATE TABLE s (id INTEGER PRIMARY KEY, region TEXT, m TEXT, amount REAL);
     INSERT INTO s VALUES (1, 'n', 'NULL', 1), (2, 'n', NULL, 10), (3, 's', '', 100);`,
  );
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT region, m, amount FROM s ORDER BY id");
  assert.equal(await page.eval(status), "3 rows");
  await page.eval(`${visibleTab}.querySelector('.result-pivot-open').click()`);
  await page.waitFor(`${dialog} != null`);
  await page.eval(`${rowBox("region")}.click()`);
  await page.eval(choose("Column field", "m"));
  await page.eval(choose("Value", "amount"));
  await page.eval(choose("Aggregate", "sum"));
  await page.eval(`[...${dialog}.querySelectorAll('.actions button')].find(b => b.textContent === 'Pivot').click()`);
  await page.waitFor(`${dialog} == null`);
  await page.waitFor(`${status} === '2 rows'`, 30_000);
  const [header, ...rows] = (await gridTsv(page)).split("\n").map((l) => l.split("\t"));
  assert.equal(header.length, 4, `region plus three columns (empty text, 'NULL', NULL): ${header}`);
  const n = rows.find((r) => r[0] === "n");
  assert.ok(!n.includes("11") && !n.includes("11.0"), `1 and 10 are not added up: ${n}`);
}
