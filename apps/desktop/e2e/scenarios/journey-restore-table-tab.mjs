// JRN-03 (low, UX). A table opened from the navigator (double click: its data tab) comes
// back after a restart as an empty SQL editor holding "SELECT * FROM t": the tab keeps the
// table's icon and title but shows no rows until Run is pressed. DBeaver reopens a table
// editor with its data loaded. (A query tab staying empty is right: re-running arbitrary SQL
// at start is not; reading a table's first rows is what the tab is for.)
// Steps: double-click a table (its rows show); close the window; start the app again.
// Expected: the restored table tab shows its rows. Actual: an empty result area.
// Suspected: apps/desktop/src/components/QueryTab.tsx (`autorun` cleared after the first run
// and persisted false, so a restored data tab never runs).
import assert from "node:assert/strict";
import { restart, status, treeRow, visibleTab } from "../journey-helpers.mjs";
import { sqliteConnection } from "../query.mjs";

const NAME = "QA jrn restore";

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_jrn_restore.db",
    id: "00000000-0000-4000-8000-0000000a6001",
    name: NAME,
    sql: "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT); INSERT INTO notes VALUES (1, 'uno'), (2, 'due');",
  });
}

export default async function (page, ctx) {
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("notes")} !== undefined`, 15_000);
  await page.eval(`${treeRow("notes")}.id = 'jrn-t'`);
  await page.clickSel("#jrn-t", { clickCount: 2 });
  await page.waitFor(`${status} === '2 rows'`, 15_000);
  // The workspace is written a moment after the last change.
  await page.waitFor(`(localStorage.getItem('platypus.workspace.v2') ?? '').includes('notes')`);
  await restart(page, ctx, { ready: `document.querySelector('.tree-row') !== null` });
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('notes')`, 15_000);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  try {
    await page.waitFor(`${status} === '2 rows'`, 10_000);
  } catch {
    assert.fail(`the restored table tab shows no rows (status "${await page.eval(status)}")`);
  }
}
