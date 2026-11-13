// QA ui: closing the window drops unsaved grid changes (and open transactions) without asking.
//
// Steps: open a table from the navigator, change a cell (the edit bar shows "1 row changed"),
// click the window's close button (top bar, or Alt+F4).
// Expected: as when the tab itself is closed, a confirmation ("This tab has unsaved changes.
// Close it and discard them?") with a way to stay; the same for a tab with an open
// transaction, which DBeaver also asks about on exit.
// Actual: the window closes at once and the change is gone. Closing a tab asks; closing the
// app never does.
// Code: no onCloseRequested handler in App.tsx (closeTab/closeTabs ask, the window does not);
// TopBar.tsx WindowControls calls `win.close()` directly.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const NAME = "QA UI exit";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const path = join(dir, "qa_ui_exit.db");
  const db = new DatabaseSync(path);
  db.exec("CREATE TABLE qa_ui_items (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO qa_ui_items VALUES (1, 'a'), (2, 'b');");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-00000000e106"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

/** The page's answer, or `null` once the window is gone (CDP never answers then). */
async function evalOrGone(page, expression, ms) {
  return Promise.race([page.eval(expression), new Promise((r) => setTimeout(() => r(null), ms))]);
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("qa_ui_items")} !== undefined`, 15_000);
  await page.eval(`${treeRow("qa_ui_items")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);

  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("ArrowRight", "ArrowRight", 39);
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor') !== null`);
  await page.type("z");
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
  await page.waitFor(`${editBar}.includes('1 row changed')`);
  assert.equal(await page.eval(`document.querySelector('.tab.active')?.textContent.includes('qa_ui_items')`), true);

  // The window's own close button, drawn in the top bar.
  await page.eval(`setTimeout(() => document.querySelector('.window-controls button.close').click(), 0); true`);
  const asked = await evalOrGone(
    page,
    `new Promise((r) => { const t = Date.now(); const f = () => document.querySelector('.modal.confirm') ? r(true) : Date.now() - t > 3000 ? r(false) : setTimeout(f, 50); f(); })`,
    5000,
  );
  if (asked === null) {
    // The window is gone: CDP calls would wait forever, and the runner reads the page on a
    // failure. Make them fail at once instead.
    const gone = () => Promise.reject(new Error("the window is closed"));
    page.send = gone;
    page.eval = gone;
    assert.fail("the window closed without asking about the unsaved change");
  }
  assert.equal(asked, true, "no confirmation about the unsaved change");
  // Stay: the change is still there.
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`${editBar}.includes('1 row changed')`);
}
