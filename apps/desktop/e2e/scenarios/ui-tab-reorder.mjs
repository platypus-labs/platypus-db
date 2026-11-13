// QA4 ui: tabs cannot be reordered.
//
// Steps: open three SQL editors (texts "select 1", "select 2", "select 3"), press the third
// tab, move the pointer over the left half of the first tab and release.
// Expected: the tab goes where it was dropped, the order becomes 3, 1, 2 (VS Code, DBeaver,
// browsers: tabs are dragged to reorder; a pinned tab stays among the pinned ones).
// Actual: nothing happens, the order stays 1, 2, 3; the only way to change it is pinning.
// Code: shell/TabBar.tsx has no drag handling at all (no pointer/drag handlers on a tab) and
// App.tsx has no function to move a tab inside `tabs`.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow } from "../query.mjs";

const NAME = "QA4 UI reorder";
const tabs = `[...document.querySelectorAll('.tab')]`;
const shown = `document.querySelector('.pane:not([hidden]) .query-tab')`;
const text = `[...(${shown}?.querySelectorAll('.cm-line') ?? [])].map(l => l.textContent).join('')`;

export function setup({ dir }) {
  sqliteConnection(dir, { file: "qa4_reorder.db", id: "00000000-0000-4000-8000-0000000e4010", name: NAME, sql: "CREATE TABLE t (id INTEGER PRIMARY KEY);" });
}

async function write(page, sql) {
  await page.waitFor(`${shown}?.querySelector('.cm-content') != null`);
  await page.eval(`${shown}.querySelector('.cm-content').focus()`);
  await page.type(sql);
  await page.waitFor(`${text} === ${JSON.stringify(sql)}`);
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${tabs}.length === 1`);
  await write(page, "select 1");
  for (const n of [2, 3]) {
    await page.key("t", "KeyT", 84, CTRL);
    await page.waitFor(`${tabs}.length === ${n}`);
    await write(page, `select ${n}`);
  }
  await page.dragEl(`${tabs}[2]`, `${tabs}[0]`, { toAt: [0.2, 0.5] });
  await page.sleep(400);
  const order = [];
  for (let i = 0; i < 3; i++) {
    await page.eval(`${tabs}[${i}].click()`);
    await page.sleep(200);
    order.push(await page.eval(text));
  }
  assert.deepEqual(order, ["select 3", "select 1", "select 2"]);
}
