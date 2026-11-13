// QA ui: a global shortcut that takes an editor's keys is not reported as a clash.
//
// Steps: Settings > Keyboard shortcuts, give "Zoom out" (Everywhere) the keys Ctrl+Shift+F,
// which "Format SQL" (SQL editor) already has.
// Expected: the row warns "also Format SQL", as it does for two actions of the same scope:
// global shortcuts run first, so Format SQL stops working in the editor.
// Actual: no warning on either row; afterwards Ctrl+Shift+F in the editor zooms out and never
// formats.
// Code: shell/ShortcutsPanel.tsx clashes() compares only actions of the same scope
// (`a.scope === action.scope`).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL, SHIFT } from "../cdp.mjs";
import { settingsFile } from "../settings.mjs";

const row = (title) =>
  `[...document.querySelectorAll('.shortcuts tr')].find(r => r.querySelector('td')?.textContent === ${JSON.stringify(title)})`;
const key = (title) => `${row(title)}?.querySelector('.shortcut-key')`;

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
}

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page, { dir }) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Keyboard shortcuts");
  await page.waitFor(`${key("Zoom out")} != null`);
  await page.eval(`${key("Zoom out")}.click(); ${key("Zoom out")}.focus()`);
  await page.waitFor(`${key("Zoom out")}.textContent.startsWith('Press')`);
  await page.key("F", "KeyF", 70, CTRL | SHIFT);
  await page.waitFor(`${key("Zoom out")}.textContent === 'Ctrl+Shift+F'`);
  await settingsFile(dir, /zoomOut = "Mod\+Shift\+F"/);
  const warned = await page.eval(`[${row("Zoom out")}, ${row("Format SQL")}].some(r => r.querySelector('.error-text') !== null)`);
  assert.equal(warned, true, "no clash shown between Zoom out and Format SQL");
}
