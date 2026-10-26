// QA ui: a tab with unsaved changes to its .sql file closes without asking.
//
// Steps: open a saved script from the Scripts panel, type in it (the tab and the toolbar
// show "•", changes not saved to the file), press Ctrl+W (or click its ×).
// Expected: "Save the changes to qa_ui_script.sql?" with Save / Discard / Cancel, as DBeaver,
// VS Code and every editor ask; closing tabs already asks for unsaved grid changes.
// Actual: the tab closes at once; the file on disk keeps the old text. Only Ctrl+Shift+T in
// the same session brings the text back; after a restart it is gone.
// Code: App.tsx closeTab asks only for `tab.unsaved` (grid edits) and `tab.txOpen`; the
// script's own dirty state is `tab.filePath !== null && tab.sql !== tab.savedSql`
// (components/QueryTab.tsx), which closeTab never checks. Same in closeTabs and on exit.
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow } from "../query.mjs";

const NAME = "QA UI script";
const shown = `document.querySelector('.pane:not([hidden]) .query-tab')`;
const text = `[...${shown}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const script = `document.querySelector('.script-row[data-path="qa_ui_script.sql"]')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_script.db",
    id: "00000000-0000-4000-8000-00000000e126",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
  mkdirSync(join(dir, "scripts", "all"), { recursive: true });
  writeFileSync(join(dir, "scripts", "all", "qa_ui_script.sql"), "select 1");
}

export default async function (page, { dir }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.waitFor(`${script} !== null`);
  await page.eval(`${script}.id = 'e2e-script'`);
  await page.clickSel("#e2e-script", { clickCount: 2 });
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.includes('qa_ui_script.sql') === true`);
  await page.waitFor(`${text} === 'select 1'`);
  await page.eval(`${shown}.querySelector('.cm-content').focus()`);
  await page.key("End", "End", 35, CTRL);
  await page.type(" as qa_ui_changed");
  await page.waitFor(`${text} === 'select 1 as qa_ui_changed'`);
  await page.waitFor(`document.querySelector('.tab.active').textContent.includes('•')`);

  await page.key("w", "KeyW", 87, CTRL);
  await page.sleep(600);
  const asked = await page.eval(`document.querySelector('.modal.confirm') !== null`);
  assert.equal(readFileSync(join(dir, "scripts", "all", "qa_ui_script.sql"), "utf8"), "select 1");
  assert.equal(asked, true, "the tab with unsaved script changes closed without asking");
}
