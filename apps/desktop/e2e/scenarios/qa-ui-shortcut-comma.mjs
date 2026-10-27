// QA ui: a recorded shortcut with the comma key is broken.
//
// Steps: Settings > Keyboard shortcuts, click the shortcut of "Query manager", press
// Ctrl+Shift+, (comma). Close the Settings and press Ctrl+Shift+, again.
// Expected: the row shows Ctrl+Shift+, and the keys open the Query manager (the default
// Settings shortcut Ctrl+, shows that the comma is a valid key).
// Actual: the row shows "Ctrl+Shift+" and the keys do nothing: settings.toml has
// "Mod+Shift+," and `shortcutsOf` splits stored shortcuts on commas, giving "Mod+Shift+" and "".
// Code: actions.ts, shortcutsOf (`own.split(/\s*,\s*/)`) and shortcutOf (no name for ",").
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL, SHIFT } from "../cdp.mjs";
import { settingsFile } from "../settings.mjs";

const key = (title) =>
  `[...document.querySelectorAll('.shortcuts tr')].find(r => r.querySelector('td')?.textContent === ${JSON.stringify(title)})?.querySelector('.shortcut-key')`;

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
}

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page, { dir }) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Keyboard shortcuts");
  await page.waitFor(`${key("Query manager")} != null`);
  await page.eval(`${key("Query manager")}.click(); ${key("Query manager")}.focus()`);
  await page.waitFor(`${key("Query manager")}.textContent.startsWith('Press')`);
  await page.key(",", "Comma", 188, CTRL | SHIFT);
  await page.waitFor(`!${key("Query manager")}.textContent.startsWith('Press')`);
  await settingsFile(dir, /query_manager = /);
  assert.equal(await page.eval(`${key("Query manager")}.textContent`), "Ctrl+Shift+,");

  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
  await page.key(",", "Comma", 188, CTRL | SHIFT);
  await page.waitFor(
    `[...document.querySelectorAll('.modal')].some(m => document.getElementById(m.getAttribute('aria-labelledby'))?.textContent === 'Query manager')`,
    3000,
  );
}
