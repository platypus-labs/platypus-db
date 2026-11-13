// QA ui: removing (or changing) an editor shortcut in the Settings does not take it away.
//
// Steps: Settings > Keyboard shortcuts, click the shortcut of "Delete line", press Backspace
// (the row shows "—"). In a SQL editor with two lines, press Ctrl+Shift+K on the first one.
// Expected: nothing happens, the shortcut was removed.
// Actual: the line is deleted. The editor's own CodeMirror keymaps (defaultKeymap,
// searchKeymap, foldKeymap) bind the same keys as the "editor" actions (Mod-/, Alt-ArrowUp,
// Shift-Mod-k, Mod-d, Mod-Alt-[...), so a removed or reassigned shortcut keeps working.
// Code: editor/SqlEditor.tsx keymap.of([... defaultKeymap, searchKeymap, foldKeymap ...])
// next to editor/actions.ts `command(...)`.
import assert from "node:assert/strict";
import { CTRL, SHIFT } from "../cdp.mjs";
import { sqliteConnection, treeRow, visibleTab, editorText } from "../query.mjs";
import { settingsFile } from "../settings.mjs";

const NAME = "QA UI unbind";
const key = (title) =>
  `[...document.querySelectorAll('.shortcuts tr')].find(r => r.querySelector('td')?.textContent === ${JSON.stringify(title)})?.querySelector('.shortcut-key')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_unbind.db",
    id: "00000000-0000-4000-8000-00000000e104",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
}

export default async function (page, { dir }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);

  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Keyboard shortcuts");
  await page.waitFor(`${key("Delete line")} != null`);
  await page.eval(`${key("Delete line")}.click(); ${key("Delete line")}.focus()`);
  await page.waitFor(`${key("Delete line")}.textContent.startsWith('Press')`);
  await page.key("Backspace", "Backspace", 8);
  await page.waitFor(`${key("Delete line")}.textContent === '—'`);
  await settingsFile(dir, /delete_line = ""/);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);

  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("select 1;\nselect 2;");
  await page.waitFor(`${editorText} === 'select 1;\\nselect 2;'`);
  await page.key("Home", "Home", 36, CTRL);
  await page.key("K", "KeyK", 75, CTRL | SHIFT);
  await page.sleep(400);
  assert.equal(await page.eval(editorText), "select 1;\nselect 2;", "the removed shortcut still deleted a line");
}
