// QA ui: closing the Settings or the command palette with Escape leaves the focus nowhere.
//
// Steps: type in a SQL editor, press Ctrl+, then Escape; press Ctrl+K then Escape; keep
// typing.
// Expected: the focus is back in the editor (modalA11y.ts promises "the focus moved inside
// while it is open and back where it was once it closes"), so typing goes on.
// Actual: the focus is on <body>; the keys typed next go nowhere until the editor is clicked.
// The Settings' search field has autoFocus, which React applies before the MutationObserver
// records the opener: the opener saved is the dialog's own input. The palette is not a .modal
// and restores nothing. The same happens after a confirmation (its button has autoFocus).
// Code: ui/modalA11y.ts prepare() (`openers.set(modal, document.activeElement)` too late),
// shell/CommandPalette.tsx.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow, visibleTab } from "../query.mjs";

const NAME = "QA UI focus";
const inEditor = `!!document.activeElement?.closest('.cm-editor')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_focus.db",
    id: "00000000-0000-4000-8000-00000000e112",
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
  await page.waitFor(inEditor);

  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings')?.contains(document.activeElement) === true`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
  await page.sleep(300);
  assert.equal(await page.eval(inEditor), true, `after the Settings the focus is on ${await page.eval(`document.activeElement?.tagName`)}`);

  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') === document.activeElement`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.palette') === null`);
  await page.sleep(300);
  assert.equal(await page.eval(inEditor), true, `after the palette the focus is on ${await page.eval(`document.activeElement?.tagName`)}`);
}
