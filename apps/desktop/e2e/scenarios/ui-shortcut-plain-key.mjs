// QA ui: a single letter is accepted as a global shortcut, and then that letter cannot be typed.
//
// Steps: Settings > Keyboard shortcuts, click the shortcut of "Activity and errors", press
// Q (no modifier). In a SQL editor type "q".
// Expected: a global shortcut needs a modifier (or a function key): plain Q is refused, as
// DBeaver and VS Code refuse it for global commands, or at least the editor keeps its text.
// Actual: Q is saved; typing "q" anywhere, the SQL editor included, opens the Activity log
// and the letter never reaches the editor.
// Code: shell/ShortcutsPanel.tsx record() accepts any `shortcutOf(e)`; the global listener
// in App.tsx runs before the editor sees the key.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow, visibleTab, editorText } from "../query.mjs";

const NAME = "QA UI plain key";
const key = (title) =>
  `[...document.querySelectorAll('.shortcuts tr')].find(r => r.querySelector('td')?.textContent === ${JSON.stringify(title)})?.querySelector('.shortcut-key')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_plain.db",
    id: "00000000-0000-4000-8000-00000000e116",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
}

/** A printable key as typing sends it: keydown with its text. */
async function typeKey(page, key, code, keyCode) {
  await page.send("Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode: keyCode, text: key });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: keyCode });
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);

  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Keyboard shortcuts");
  await page.waitFor(`${key("Activity and errors")} != null`);
  await page.eval(`${key("Activity and errors")}.click(); ${key("Activity and errors")}.focus()`);
  await page.waitFor(`${key("Activity and errors")}.textContent.startsWith('Press')`);
  await typeKey(page, "q", "KeyQ", 81);
  await page.sleep(300);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);

  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("select ");
  await page.waitFor(`${editorText} === 'select '`);
  await typeKey(page, "q", "KeyQ", 81);
  await page.sleep(500);
  assert.equal(await page.eval(`document.querySelector('.modal.activity') === null`), true, "typing q opened the Activity log");
  assert.equal(await page.eval(editorText), "select q");
}
