// QA ui: closing a tab jumps to the last tab, and a reopened tab comes back at the end.
//
// Steps: four SQL editors A B C D; activate B and press Ctrl+W; then Ctrl+Shift+T.
// Expected: as in browsers, VS Code and DBeaver, closing B shows its neighbour C, and
// reopening puts B back in its place, between A and C.
// Actual: closing B shows D (the last tab), and the reopened B is added after D.
// Code: App.tsx closeTab (`same.at(-1)`) and reopenTab (addTab appends).
import assert from "node:assert/strict";
import { CTRL, SHIFT } from "../cdp.mjs";
import { sqliteConnection, treeRow } from "../query.mjs";

const NAME = "QA UI order";
const tabs = `[...document.querySelectorAll('.tab')]`;
const shown = `document.querySelector('.pane:not([hidden]) .query-tab')`;
const text = `[...${shown}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_order.db",
    id: "00000000-0000-4000-8000-00000000e119",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
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
  await write(page, "select 'A'");
  for (const [n, letter] of [[2, "B"], [3, "C"], [4, "D"]]) {
    await page.key("t", "KeyT", 84, CTRL);
    await page.waitFor(`${tabs}.length === ${n}`);
    await write(page, `select '${letter}'`);
  }
  await page.eval(`${tabs}[1].click()`);
  await page.waitFor(`${text} === "select 'B'"`);
  await page.key("w", "KeyW", 87, CTRL);
  await page.waitFor(`${tabs}.length === 3`);
  await page.sleep(200);
  assert.equal(await page.eval(text), "select 'C'", "the tab shown after closing B");

  await page.key("T", "KeyT", 84, CTRL | SHIFT);
  await page.waitFor(`${tabs}.length === 4`);
  await page.waitFor(`${text} === "select 'B'"`);
  assert.equal(await page.eval(`${tabs}.findIndex(t => t.classList.contains('active'))`), 1, "where the reopened tab went");
}
