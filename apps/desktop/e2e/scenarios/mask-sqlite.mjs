// Masking for screen sharing: with the setting on (settings.toml), the top bar says so, the
// value panel hides an email but shows a name, and Ctrl+Alt+M turns it off, after which the
// email shows. Copy keeps the real value all along.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { ALT, CTRL } from "../cdp.mjs";

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const panel = `(${visibleTab}?.querySelector('.value-panel')?.textContent ?? '')`;
const marker = `document.querySelector('.topbar-masked')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "people.db"));
  db.exec("CREATE TABLE people (id INTEGER PRIMARY KEY, full_name TEXT, email TEXT); INSERT INTO people VALUES (1, 'Giulia Rossi', 'giulia@example.com');");
  db.close();
  writeFileSync(join(dir, "settings.toml"), "[safety]\nmask = true\n\n[grid]\nvalue_panel = true\n");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000921"
name = "People db"
driver = "sqlite"
database = '${join(dir, "people.db")}'
save_password = false
`,
  );
}

/** Focuses the cell of the first row in result column `col` (0-based). */
async function focusCell(page, col) {
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < col; i++) await page.key("ArrowRight", "ArrowRight", 39);
}

export default async function (page) {
  await page.waitFor(`${marker} !== null`, 15_000);
  assert.match(await page.eval(`${marker}.textContent`), /Masked/);

  await page.waitFor(`${row("People db")} !== undefined`);
  await page.eval(`${row("People db")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("people")} !== undefined`, 15_000);
  await page.eval(`${row("people")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${status} === '1 row'`, 15_000);

  await focusCell(page, 2);
  await page.waitFor(`${panel}.includes('Masked while the screen may be shared')`, 15_000);
  assert.ok(!(await page.eval(`${panel}.includes('giulia@example.com')`)));
  await focusCell(page, 1);
  await page.waitFor(`${panel}.includes('Giulia Rossi')`, 15_000);

  // The shortcut turns it off: the marker goes, the email shows.
  await page.key("m", "KeyM", 77, CTRL | ALT);
  await page.waitFor(`${marker} === null`, 15_000);
  await focusCell(page, 2);
  await page.waitFor(`${panel}.includes('giulia@example.com')`, 15_000);
}
