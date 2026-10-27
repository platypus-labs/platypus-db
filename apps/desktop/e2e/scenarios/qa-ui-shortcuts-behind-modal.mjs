// QA ui: the global shortcuts still act on the window behind an open dialog.
//
// Steps: open two SQL editors, open the Settings (Ctrl+,), press Ctrl+W, then Ctrl+T, then
// Ctrl+K while the Settings are on top.
// Expected: a modal dialog takes the keyboard; Ctrl+W/Ctrl+T do nothing behind it and the
// palette does not open over it (as they already wait while a confirmation is asked).
// Actual: Ctrl+W closes the active tab behind the Settings, Ctrl+T opens a new one, and
// Ctrl+K opens the command palette on top of the Settings.
// Code: App.tsx, the global keydown listener only checks `isAsking()`, not open dialogs.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow } from "../query.mjs";

const NAME = "QA UI modal";
const tabs = `document.querySelectorAll('.tab').length`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_modal.db",
    id: "00000000-0000-4000-8000-00000000e101",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${tabs} === 1`);
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`${tabs} === 2`);

  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.waitFor(`document.querySelector('.modal.settings').contains(document.activeElement)`);

  // Give each key the time to act, if it does.
  await page.key("w", "KeyW", 87, CTRL);
  await page.sleep(500);
  assert.equal(await page.eval(tabs), 2, "Ctrl+W closed a tab behind the Settings");
  await page.key("t", "KeyT", 84, CTRL);
  await page.sleep(500);
  assert.equal(await page.eval(tabs), 2, "Ctrl+T opened a tab behind the Settings");
  await page.key("k", "KeyK", 75, CTRL);
  await page.sleep(500);
  assert.equal(await page.eval(`document.querySelector('.palette') === null`), true, "the palette opened over the Settings");
  assert.equal(await page.eval(`document.querySelector('.modal.settings') !== null`), true);
}
