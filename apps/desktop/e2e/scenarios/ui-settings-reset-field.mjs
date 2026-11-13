// QA ui: Reset on a number or text setting leaves the old value in its field.
//
// Steps: Settings > Appearance, type 16 in "SQL editor font size" and press Enter; click its
// Reset. Then Settings > Results, type %d/%m/%Y in "Date format", leave the field, click Reset,
// then focus and leave the field again.
// Expected: after Reset the field shows the default (13; empty), and leaving the field
// saves nothing.
// Actual: the font size field still shows 16 while the editor is back to 13px; the date field
// still shows %d/%m/%Y, and focusing and leaving it saves %d/%m/%Y again, undoing the Reset.
// Code: components/SettingsDialog.tsx, NumberInput keeps its own text state
// (`useState(String(props.value))`) and the text control is uncontrolled (`defaultValue`):
// neither follows a value changed from outside.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { settingsFile, settingsLack } from "../settings.mjs";

const setting = (label) =>
  `[...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === ${JSON.stringify(label)})`;
const reset = (label) => `[...${setting(label)}.querySelectorAll('button')].find(b => b.textContent === 'Reset')`;
const cssVar = (name) => `getComputedStyle(document.documentElement).getPropertyValue(${JSON.stringify(name)}).trim()`;

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
}

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page, { dir }) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Appearance", "Language and text");

  const font = `${setting("SQL editor font size")}.querySelector('input')`;
  await page.eval(`${font}.focus(); ${font}.select()`);
  await page.type("16");
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${cssVar("--editor-font-size")} === '16px'`);
  await settingsFile(dir, /editor_font_size = 16/);
  await page.eval(`${reset("SQL editor font size")}.click()`);
  await page.waitFor(`${cssVar("--editor-font-size")} === '13px'`);
  await settingsLack(dir, /editor_font_size/);
  assert.equal(await page.eval(`${font}.value`), "13", "the font size field after Reset");

  await page.settingsTab("Results");
  const date = `${setting("Date format")}.querySelector('input')`;
  await page.eval(`${date}.focus()`);
  await page.type("%d/%m/%Y");
  await page.eval(`${date}.blur()`);
  await settingsFile(dir, /date_format = "%d\/%m\/%Y"/);
  await page.eval(`${reset("Date format")}.click()`);
  await settingsLack(dir, /date_format/);
  // Focusing and leaving the field must not save anything.
  await page.eval(`${date}.focus()`);
  await page.eval(`${date}.blur()`);
  await page.sleep(500);
  await settingsLack(dir, /date_format/);
  assert.equal(await page.eval(`${date}.value`), "", "the date format field after Reset");
}
