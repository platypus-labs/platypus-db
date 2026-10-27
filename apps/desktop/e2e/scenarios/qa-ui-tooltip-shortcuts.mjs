// QA ui: tooltips name the default shortcut even after it was changed in the Settings.
//
// Steps: Settings > Keyboard shortcuts, give "Close tab" the keys Ctrl+Shift+W and
// "Settings" the keys Ctrl+Shift+O. Hover a tab's × and the top bar's gear.
// Expected: "Close tab (Ctrl+Shift+W)" and "Settings (Ctrl+Shift+O)", as the navigator
// button and the palette already follow the user's shortcuts.
// Actual: "Close tab (Ctrl+W)" and "Settings (Ctrl+,)", written into the texts.
// Code: shell/TabBar.tsx `t("Close tab (Ctrl+W)")`, shell/TopBar.tsx `t("Settings (Ctrl+,)")`
// (compare toggleSidebarTitle() in shell/sidebar.ts, which uses shortcutLabel).
import assert from "node:assert/strict";
import { CTRL, SHIFT } from "../cdp.mjs";
import { sqliteConnection, treeRow } from "../query.mjs";
import { settingsFile } from "../settings.mjs";

const NAME = "QA UI tooltips";
const key = (title) =>
  `[...document.querySelectorAll('.shortcuts tr')].find(r => r.querySelector('td')?.textContent === ${JSON.stringify(title)})?.querySelector('.shortcut-key')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_tooltips.db",
    id: "00000000-0000-4000-8000-00000000e124",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
}

async function record(page, title, k, code, keyCode, modifiers, label) {
  await page.waitFor(`${key(title)} != null`);
  await page.eval(`${key(title)}.click(); ${key(title)}.focus()`);
  await page.waitFor(`${key(title)}.textContent.startsWith('Press')`);
  await page.key(k, code, keyCode, modifiers);
  await page.waitFor(`${key(title)}.textContent === ${JSON.stringify(label)}`);
}

export default async function (page, { dir }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`document.querySelectorAll('.tab').length === 1`);

  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Keyboard shortcuts");
  await record(page, "Close tab", "W", "KeyW", 87, CTRL | SHIFT, "Ctrl+Shift+W");
  await record(page, "Settings", "O", "KeyO", 79, CTRL | SHIFT, "Ctrl+Shift+O");
  await settingsFile(dir, /settings = "Mod\+Shift\+O"/);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);

  assert.deepEqual(
    [
      await page.eval(`document.querySelector('.tab button[aria-label="Close tab"]').title`),
      await page.eval(`document.querySelector('.topbar button[aria-label="Settings"]').title`),
    ],
    ["Close tab (Ctrl+Shift+W)", "Settings (Ctrl+Shift+O)"],
  );
}
