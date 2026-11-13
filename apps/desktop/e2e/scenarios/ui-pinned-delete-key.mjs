// QA ui: the Delete key closes a pinned tab.
//
// Steps: pin a tab (tab menu > Pin tab), focus it in the tab bar, press Delete.
// Expected: a pinned tab has no close button and "Close others"/"Close all" keep it; the
// Delete key should keep it too (or ask), Unpin first.
// Actual: the pinned tab closes.
// Code: shell/TabBar.tsx onKey, `else if (e.key === "Delete") props.onClose(tab)` without a
// check for `tab.pinned`.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow } from "../query.mjs";

const NAME = "QA UI pinned";
const tabs = `[...document.querySelectorAll('.tab')]`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_pinned.db",
    id: "00000000-0000-4000-8000-00000000e120",
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
  await page.waitFor(`${tabs}.length === 2`);
  await page.eval(`${tabs}[0].dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 300, clientY: 60 }))`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu [role=menuitem]')].find(b => b.textContent.startsWith('Pin tab')).click()`);
  await page.waitFor(`${tabs}[0].classList.contains('pinned')`);
  await page.eval(`${tabs}[0].click()`);
  await page.waitFor(`${tabs}[0].classList.contains('active')`);
  await page.eval(`${tabs}[0].focus()`);
  await page.key("Delete", "Delete", 46);
  await page.sleep(400);
  assert.equal(await page.eval(`${tabs}.filter(t => t.classList.contains('pinned')).length`), 1, "the pinned tab was closed by Delete");
}
