// QA4 ui: with the Cancel button hidden in Settings, a running statement cannot be stopped.
//
// Steps: Settings > Toolbar, untick "Cancel button" (show_cancel = false); run a long
// statement (a recursive query over hundreds of millions of rows) with Ctrl+Enter.
// Expected: some way to stop it while it runs: the button shown while a statement runs, a
// "Cancel" command in the palette (Ctrl+K), or a shortcut (Esc in the editor, as DBeaver).
// Actual: the toolbar has no Cancel, the status bar and the palette have no such action and no
// shortcut is listed in Settings > Keyboard shortcuts; the statement runs to its end (or
// the tab is closed).
// Code: components/QueryTab.tsx: `cancel()` is only reachable from the toolbar button wrapped
// in `toolbar.visible.cancel`; there is no registered action for it.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow, visibleTab } from "../query.mjs";

export function setup({ dir }) {
  sqliteConnection(dir, { file: "qa4_cancel.db", id: "00000000-0000-4000-8000-0000000e4011", name: "QA4 hidden stop", sql: "CREATE TABLE a (id INTEGER PRIMARY KEY);" });
  writeFileSync(join(dir, "settings.toml"), "[toolbar]\nshow_cancel = false\n");
}

export default async function (page) {
  await page.waitFor(`${treeRow("QA4 hidden stop")} !== undefined`);
  await page.eval(`${treeRow("QA4 hidden stop")}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("with recursive c(x) as (select 1 union all select x+1 from c where x < 400000000) select count(*) from c");
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes('from c')`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`(${visibleTab}.querySelector('.statusbar span')?.textContent ?? '').includes('Executing')`);
  // Is there a cancel control in the tab, or an action in the palette?
  const inTab = await page.eval(`[...${visibleTab}.querySelectorAll('button')].some(b => /cancel/i.test(b.getAttribute('aria-label') ?? '') && !b.disabled && b.getClientRects().length > 0)`);
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('[role=dialog]') !== null`);
  await page.type("cancel");
  await page.sleep(500);
  const inPalette = await page.eval(`/cancel/i.test(document.querySelector('[role=dialog]')?.innerText ?? '')`);
  assert.ok(inTab || inPalette, "a statement is running and nothing in the window can cancel it");
}
