// QA ui: the Reset tooltip of a select setting shows the internal value, not its label.
//
// Steps: Settings > Appearance, hover the Reset of "Corners", "Grid rows", "Title bar",
// "Theme" or "Language" (set them to another value first so Reset shows).
// Expected: "Default: Rounded", "Default: Normal", "Default: In the top bar", "Default:
// System" (and in Italian "Predefinito: Arrotondati", ...).
// Actual: "Default: rounded", "Default: normal", "Default: app", "Default: system": the
// stored value, in English whatever the language.
// Code: components/SettingsDialog.tsx SettingRow, `t("Default: {value}", { value:
// String(def.default) })` instead of the option's translated label.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";

const setting = (label) =>
  `[...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === ${JSON.stringify(label)})`;
const resetTitle = (label) => `[...${setting(label)}.querySelectorAll('button')].find(b => b.textContent === 'Reset').title`;

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "en"\nradius = "square"\ndensity = "compact"\ntitle_bar = "system"\n');
}

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Appearance", "Density");
  const density = [await page.eval(resetTitle("Corners")), await page.eval(resetTitle("Grid rows"))];
  await page.settingsTab("Appearance", "Window");
  const window = await page.eval(resetTitle("Title bar"));
  assert.deepEqual([...density, window], ["Default: Rounded", "Default: Normal", "Default: In the top bar"]);
}
