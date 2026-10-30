// QA ui: integer settings accept fractions, and Format SQL then fails with a raw error.
//
// Steps: Settings > SQL editor, type 2.5 in "Indentation when formatting (spaces)" and
// leave the field. In a SQL editor press Ctrl+Shift+F.
// Expected: the field refuses 2.5 (step 1: back to 2, or rounded), and formatting works.
// Actual: 2.5 is saved (format_indent = 2.5 in settings.toml) and every Format SQL fails with
// "invalid args `settings` for command `format_sql`: invalid type: floating point `2.5`,
// expected usize". The same for "LIMIT of Generate SELECT" (10.5), the font sizes (13.7) and
// the other number settings.
// Code: components/SettingsDialog.tsx NumberInput.commit checks min/max but not `step`.
import assert from "node:assert/strict";
import { CTRL, SHIFT } from "../cdp.mjs";
import { sqliteConnection, treeRow, visibleTab, editorText } from "../query.mjs";

const NAME = "QA UI integer";
const setting = (label) =>
  `[...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_integer.db",
    id: "00000000-0000-4000-8000-00000000e114",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);

  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("SQL editor", "Formatting");
  const indent = `${setting("Indentation when formatting (spaces)")}.querySelector('input')`;
  await page.eval(`${indent}.focus(); ${indent}.select()`);
  await page.type("2.5");
  await page.eval(`${indent}.blur()`);
  await page.sleep(300);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);

  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("select a from qa_ui_t where a = 1");
  await page.waitFor(`${editorText} === 'select a from qa_ui_t where a = 1'`);
  await page.key("F", "KeyF", 70, CTRL | SHIFT);
  await page.waitFor(`${editorText} !== 'select a from qa_ui_t where a = 1' || document.querySelector('.toast') !== null`, 5000);
  assert.equal(await page.eval(`document.querySelector('.toast')?.textContent ?? null`), null, "Format SQL failed");
  assert.match(await page.eval(editorText), /^SELECT/);
}
