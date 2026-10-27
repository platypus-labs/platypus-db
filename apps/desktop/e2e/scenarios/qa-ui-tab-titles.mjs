// QA ui: SQL editors of the same connection all have the same title.
//
// Steps: double-click a connection, then Ctrl+T twice.
// Expected: tabs that can be told apart, as DBeaver's "<conn> Script", "<conn> Script-1",
// or a number/first words of the SQL.
// Actual: three tabs all titled "QA UI titles"; with the bar grouped by connection they are
// three identical icons-and-name chips. Only clicking each shows which is which.
// Code: App.tsx tabTitle (the connection name when the kind has no label) and the query tab
// kind's label (empty for a plain SQL editor).
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow } from "../query.mjs";

const NAME = "QA UI titles";
const tabs = `[...document.querySelectorAll('.tab')]`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_titles.db",
    id: "00000000-0000-4000-8000-00000000e121",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${tabs}.length === 1`);
  await page.key("t", "KeyT", 84, CTRL);
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`${tabs}.length === 3`);
  const titles = await page.eval(`${tabs}.map(t => t.textContent.replace('×', '').trim())`);
  assert.equal(new Set(titles).size, 3, `tab titles: ${titles.join(" | ")}`);
}
