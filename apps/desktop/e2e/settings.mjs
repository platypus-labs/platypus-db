// Reading the settings.toml the app writes in a scenario's config directory.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

/** Reads settings.toml every 100 ms until `done(text)` holds or 5 s pass, and returns the last
 *  text read: `null` while the file cannot be read, as it is not written yet. */
async function readSettings(dir, done) {
  const deadline = Date.now() + 5000;
  let text = null;
  while (!done(text) && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 100));
    try {
      text = readFileSync(join(dir, "settings.toml"), "utf8");
    } catch {
      // Not written yet.
    }
  }
  return text;
}

/** Waits until settings.toml matches `pattern`. */
export async function settingsFile(dir, pattern) {
  const text = await readSettings(dir, (t) => t !== null && pattern.test(t));
  assert.match(text ?? "", pattern);
}

/** Waits until settings.toml has been read and no longer matches `pattern`: a key that is
 *  removed. A file that cannot be read yet never counts as "without the key". */
export async function settingsLack(dir, pattern) {
  const text = await readSettings(dir, (t) => t !== null && !pattern.test(t));
  assert.notEqual(text, null, "settings.toml was never written");
  assert.doesNotMatch(text, pattern);
}

/** Page expressions for the Settings dialog, to evaluate in the app. */

/** The row of the setting with this full key (`layout.navigator_side`). */
export const settingRow = (key) => `document.querySelector('.modal.settings .setting[data-key="${key}"]')`;

/** The Reset button of that setting's row. */
export const settingReset = (key) => `[...${settingRow(key)}.querySelectorAll('button')].find(b => b.textContent === 'Reset')`;

/** The sub-tab of a settings page with this label. */
export const settingsSubTab = (label) => `document.querySelector('.modal.settings [role=tab][data-subtab=${JSON.stringify(label)}]')`;
