// QA ui: what was typed in the editor just before the window closes (or reloads) is lost.
//
// Steps: in a SQL editor type a statement, then close the window (or reload) within about
// 400 ms of the last key. Start again.
// Expected: the restored tab has the whole text.
// Actual: the restored tab has the text as it was before the last keys: the workspace is
// saved to localStorage by a 400 ms debounce and nothing flushes it when the page goes away
// (no beforeunload/pagehide or close-requested handler). Seen with the real window close
// button as well as with a reload.
// Code: workspace.ts saveWorkspace (setTimeout 400) and App.tsx, no flush on exit.
import assert from "node:assert/strict";
import { sqliteConnection, treeRow, visibleTab, editorText } from "../query.mjs";

const NAME = "QA UI flush";

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_flush.db",
    id: "00000000-0000-4000-8000-00000000e105",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("select 1");
  await page.waitFor(`${editorText} === 'select 1'`);
  // Saved by now.
  await page.waitFor(`(localStorage.getItem('platypus.workspace.v2') ?? '').includes('select 1')`);
  await page.type(" as qa_ui_last_words");
  await page.waitFor(`${editorText} === 'select 1 as qa_ui_last_words'`);
  // The user closes the window right after typing.
  await page.eval(`location.reload()`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 30_000);
  await page.waitFor(`${editorText} !== ''`);
  assert.equal(await page.eval(editorText), "select 1 as qa_ui_last_words", "the last keys typed were lost");
}
