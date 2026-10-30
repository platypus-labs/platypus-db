// Sensitive columns (group Q): the rules on names say `email` and `phone_model` are sensitive;
// in the table's tab the user marks `phone_model` not sensitive and `private_note` sensitive,
// and the marks are kept. With masking on, the data viewer's value panel hides `email` and
// `private_note` and shows `phone_model`; the header says which columns are sensitive. An export
// of the result names the sensitive columns and masks them, or leaves them out; going back to
// the rules forgets a mark.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { answerSave, button, fill } from "../qa-xfer.mjs";

const NAME = "Sensitive db";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const objectTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;
const dataTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${dataTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const panel = `(${dataTab}?.querySelector('.value-panel')?.textContent ?? '')`;
/** The Sensitive select of column `name` in the table's tab. */
const sensitiveOf = (name) => `${objectTab}?.querySelector('select[aria-label=${JSON.stringify(`Sensitive: ${name}`)}]')`;
const exportDialog = `[...document.querySelectorAll('.modal')].find(m => m.querySelector('h2')?.textContent === 'Export result')`;
const exported = `[...${dataTab}.querySelectorAll('span')].find(s => /^(Exported|Export failed|Export cancelled)/.test(s.textContent))`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "people.db"));
  db.exec(`CREATE TABLE people (id INTEGER PRIMARY KEY, full_name TEXT, email TEXT, phone_model TEXT, private_note TEXT);
           INSERT INTO people VALUES (1, 'Giulia Rossi', 'giulia@example.com', 'Pixel 9', 'owes 20 euros'),
                                     (2, 'Marco Bianchi', NULL, 'iPhone', NULL);`);
  db.close();
  writeFileSync(join(dir, "settings.toml"), "[safety]\nmask = true\n\n[grid]\nvalue_panel = true\n");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000a5e01"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "people.db")}'
save_password = false
`,
  );
}

/** Chooses `value` in the Sensitive select of column `name` and waits for it to stay. */
async function mark(page, name, value) {
  await page.eval(fill(sensitiveOf(name), value));
  await page.waitFor(`${sensitiveOf(name)}.value === ${JSON.stringify(value)}`, 5000);
}

/** Focuses the cell of the first row in result column `col` (0-based). */
async function focusCell(page, col) {
  await page.eval(`${dataTab}.querySelector('.grid-scroll').focus()`);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < col; i++) await page.key("ArrowRight", "ArrowRight", 39);
}

/** Exports the data viewer's loaded rows to `path`, the sensitive columns as `choice`. */
async function exportAs(page, path, choice) {
  await page.eval(`[...${dataTab}.querySelectorAll('button.link')].find(b => b.textContent === 'Export…').click()`);
  await page.waitFor(`${exportDialog} !== undefined`);
  const field = `${exportDialog}.querySelector('[data-field="sensitive"]')`;
  await page.waitFor(`${field} != null`, 5000);
  const label = await page.eval(`${field}.closest('label').textContent`);
  assert.match(label, /Sensitive columns \(email, private_note\)/, label);
  // Masking is on: a new export masks them at first.
  assert.equal(await page.eval(`${field}.value`), "mask");
  await page.eval(fill(field, choice));
  await page.eval(`${exportDialog}.querySelectorAll('fieldset.choices input')[1]?.click()`);
  await answerSave(page, path);
  await page.eval(`${button(exportDialog, "Export…")}.click()`);
  await page.waitFor(`${exported} !== undefined`, 30_000);
  const text = await page.eval(`${exported}.textContent`);
  await page.eval(`${exported}.querySelector('button')?.click()`);
  assert.match(text, /^Exported/, text);
  return readFileSync(path, "utf8").replaceAll("\r\n", "\n");
}

export default async function (page, { dir }) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("people")} !== undefined`, 15_000);

  // The table's tab: the rules' answer for each column, then the user's marks over it.
  await page.eval(`${row("people")}.querySelector('.tree-label').id = 'e2e-row'`);
  const r = await page.rect("#e2e-row");
  await page.eval(`document.getElementById('e2e-row').removeAttribute('id')`);
  await page.click(r.x + 5, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Open details')).click()`);
  await page.waitFor(`${objectTab} !== undefined`, 10_000);
  await page.eval(`[...${objectTab}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith('Columns')).click()`);
  await page.waitFor(`${sensitiveOf("private_note")} != null`, 10_000);
  const byRules = async (name) => page.eval(`${sensitiveOf(name)}.selectedOptions[0].textContent`);
  assert.equal(await byRules("email"), "Sensitive, by the rules");
  assert.equal(await byRules("phone_model"), "Sensitive, by the rules");
  assert.equal(await byRules("full_name"), "Not sensitive, by the rules");
  assert.equal(await byRules("private_note"), "Not sensitive, by the rules");
  await mark(page, "phone_model", "no");
  await mark(page, "private_note", "yes");

  // The data viewer, masking on: the marks count over the rules.
  await page.eval(`[...${objectTab}.querySelectorAll('button')].find(b => b.textContent === 'Open data').click()`);
  await page.waitFor(`${status} === '2 rows'`, 15_000);
  const expect = [
    [1, "Giulia Rossi", true],
    [2, "giulia@example.com", false],
    [3, "Pixel 9", true],
    [4, "owes 20 euros", false],
  ];
  for (const [col, value, visible] of expect) {
    await focusCell(page, col);
    if (visible) await page.waitFor(`${panel}.includes(${JSON.stringify(value)})`, 10_000);
    else {
      await page.waitFor(`${panel}.includes('Masked while the screen may be shared')`, 10_000);
      assert.ok(!(await page.eval(`${panel}.includes(${JSON.stringify(value)})`)), `column ${col} shows ${value}`);
    }
  }
  // The header's tooltip (the grid's title as the mouse moves over it) names sensitive columns.
  await page.eval(`${dataTab}.querySelector('.grid-scroll').id = 'e2e-grid'`);
  const g = await page.rect("#e2e-grid");
  await page.eval(`document.getElementById('e2e-grid').removeAttribute('id')`);
  const titles = new Set();
  for (let x = g.x + 10; x < g.x + g.w - 10; x += 30) {
    await page.mouse("mouseMoved", x, g.y + 10);
    titles.add(await page.eval(`${dataTab}.querySelector('.grid-scroll').title`));
  }
  assert.ok([...titles].some((t) => t.includes("Sensitive column")), `a header says it: ${JSON.stringify([...titles])}`);

  // Exports: masked (a NULL stays empty), then left out.
  const masked = await exportAs(page, join(dir, "masked.csv"), "mask");
  assert.equal(masked, "id,full_name,email,phone_model,private_note\n1,Giulia Rossi,••••••,Pixel 9,••••••\n2,Marco Bianchi,,iPhone,\n");
  const left = await exportAs(page, join(dir, "left.csv"), "omit");
  assert.equal(left, "id,full_name,phone_model\n1,Giulia Rossi,Pixel 9\n2,Marco Bianchi,iPhone\n");

  // Back to the rules: phone_model is sensitive again, kept after the tab is drawn again.
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.querySelector('.tab-icon') && t.textContent.startsWith('people') && t !== document.querySelector('.tab.active')).click()`);
  await page.waitFor(`${sensitiveOf("phone_model")} != null`, 10_000);
  await mark(page, "phone_model", "");
  assert.equal(await byRules("phone_model"), "Sensitive, by the rules");
  assert.equal(await page.eval(`${sensitiveOf("private_note")}.value`), "yes", "the other mark stays");
}
