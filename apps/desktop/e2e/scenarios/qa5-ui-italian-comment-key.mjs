// QA5 ui: "Toggle comment" (Ctrl+/) does nothing on keyboards where `/` is a shifted key.
//
// Steps: SQL editor with `select 1`; on an Italian, German, Spanish or French layout `/` is
// Shift+7, so the shortcut is pressed as Ctrl+Shift+7 and the page receives key "/" with ctrl and
// shift held. (Control: on a US layout Ctrl+/ comments the line.)
// Expected: the line is commented, as the Settings list the shortcut as Ctrl+/ (docs/user/SHORTCUTS.md).
// Actual: nothing happens; the shortcut cannot be used on those layouts, not even by rebinding
// it to the same key (the Settings record "Ctrl+Shift+/", which the editor does not run either).
// Code: apps/desktop/src/actions.ts `matches` compares `e.shiftKey` with the shortcut's Shift for
// printable keys (a key that needs Shift to be typed reports it), and the editor's own keymap
// (editor/actions.ts `toggle_comment`, "Mod+/") has the same limit.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, SHIFT } from "../cdp.mjs";

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const treeRow = (label) => `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  new DatabaseSync(join(dir, "a.db")).close();
  writeFileSync(join(dir, "connections.toml"), `version = 1\n[[connections]]\nid = "00000000-0000-4000-8000-000000000e53"\nname = "Slash db"\ndriver = "sqlite"\ndatabase = '${join(dir, "a.db")}'\nsave_password = false\n`);
}

const press = async (page, key, code, keyCode, modifiers) => {
  for (const type of ["rawKeyDown", "keyUp"]) await page.send("Input.dispatchKeyEvent", { type, key, code, windowsVirtualKeyCode: keyCode, modifiers });
};

export default async function (page) {
  await page.eval(`${treeRow("Slash db")}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("select 1");
  const text = () => page.eval(`${visibleTab}.querySelector('.cm-content').textContent`);
  await press(page, "/", "Slash", 191, CTRL);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent === '-- select 1'`);
  await press(page, "/", "Slash", 191, CTRL);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent === 'select 1'`);
  await press(page, "/", "Digit7", 55, CTRL | SHIFT);
  await page.sleep(500);
  assert.equal(await text(), "-- select 1", "Ctrl+Shift+7 (the `/` of an Italian keyboard) comments the line");
}
