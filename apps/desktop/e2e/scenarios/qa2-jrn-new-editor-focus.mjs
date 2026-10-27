// JRN-14 (medium, keyboard). Ctrl+T opens a new SQL editor but does not put the cursor in
// it: what the user types next goes nowhere (or into whatever had the focus), and Ctrl+Enter
// runs nothing. With the mouse one clicks into the editor first; with the keyboard there is
// no way into it short of tabbing through the title bar, the rail and the navigator.
// Steps: open a SQL editor on a connection; click a cell of its result (or anywhere outside
// the editor); Ctrl+T; type "select 1".
// Expected: the new editor has the focus and holds "select 1". Actual: the focus stays
// outside, the editor is empty.
// Suspected: apps/desktop/src/App.tsx `tab.new` (~1200) opens the tab without focusing its
// editor; apps/desktop/src/editor/SqlEditor.tsx focuses a view only on its own events.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, status, treeRow, visibleTab } from "../query.mjs";

const NAME = "QA jrn new editor";

export function setup({ dir }) {
  sqliteConnection(dir, { file: "qa_jrn_focus.db", id: "00000000-0000-4000-8000-0000000a8007", name: NAME, sql: "CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);" });
}

export default async function (page) {
  await page.eval(`${treeRow(NAME)}.id = 'jrn-c'`);
  await page.clickSel("#jrn-c", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("select * from t");
  await page.key("Escape", "Escape", 27);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${status} === '1 row'`, 10_000);
  // The user clicks the result.
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').id = 'jrn-grid'`);
  const r = await page.rect("#jrn-grid");
  await page.click(r.x + 60, r.y + 40);
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`document.querySelectorAll('.tab').length === 2 && ${visibleTab}?.querySelector('.cm-content') != null`, 10_000);
  await page.sleep(300);
  await page.type("select 1");
  await page.sleep(200);
  const text = await page.eval(`${visibleTab}.querySelector('.cm-content').textContent`);
  const focus = await page.eval(`document.activeElement === document.body ? 'body' : document.activeElement.className`);
  assert.equal(text, "select 1", `the new editor did not get the keys (focus: ${focus})`);
}
