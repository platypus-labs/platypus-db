// The application shell: Ctrl+Shift+T reopens a closed tab, the wheel scrolls the tab bar,
// the command palette runs actions and opens tables, shortcuts can be changed in the
// Settings, and toolbar buttons hidden.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, SHIFT } from "../cdp.mjs";

const NAME = "E2E shell";
const tabCount = `document.querySelectorAll('.tab').length`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const settingRow = (label) =>
  `[...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const path = join(dir, "shell.db");
  const db = new DatabaseSync(path);
  db.exec("CREATE TABLE inventory_items (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO inventory_items VALUES (1, 'bolt');");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000008"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

async function settingsFile(dir, pattern) {
  const deadline = Date.now() + 5000;
  let text = "";
  while (!pattern.test(text) && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 100));
    try {
      text = readFileSync(join(dir, "settings.toml"), "utf8");
    } catch {
      // Not written yet.
    }
  }
  assert.match(text, pattern);
}

async function palette(page, query) {
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') !== null`);
  await page.type(query);
  await page.waitFor(`document.querySelector('.palette li.selected') !== null`);
}

export default async function (page, { dir, shots }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${tabCount} === 1`);

  // The toolbar says where statements run; the tab shows its kind.
  await page.waitFor(`${visibleTab}.querySelector('.conn-chip')?.textContent === 'E2E shell · main'`);
  assert.equal(await page.eval(`!!document.querySelector('.tab.active .tab-icon svg')`), true);

  // Closed tabs come back with their SQL.
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("SELECT 'kept'");
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`${tabCount} === 2`);
  await page.key("Tab", "Tab", 9, CTRL | SHIFT);
  await page.key("w", "KeyW", 87, CTRL);
  await page.waitFor(`${tabCount} === 1`);
  await page.key("T", "KeyT", 84, CTRL | SHIFT);
  await page.waitFor(`${tabCount} === 2`);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent === "SELECT 'kept'"`);

  // The wheel scrolls the tab bar sideways once tabs overflow it.
  for (let i = 0; i < 12; i++) await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`${tabCount} === 14`);
  await page.eval(`document.querySelector('.tabs').scrollLeft = 0`);
  await page.eval(`document.querySelector('.tabs').dispatchEvent(new WheelEvent('wheel', { deltaY: 240, bubbles: true }))`);
  await page.waitFor(`document.querySelector('.tabs').scrollLeft > 0`);

  // The palette runs actions and opens tables of the loaded catalogs.
  await palette(page, "settings");
  assert.equal(await page.eval(`getComputedStyle(document.querySelector('.palette input')).height`), "44px");
  assert.match(await page.eval(`document.querySelector('.palette li.selected').textContent`), /Settings/);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await palette(page, "invitems");
  assert.match(await page.eval(`document.querySelector('.palette li.selected').textContent`), /inventory_items/);
  if (shots) await page.screenshot(join(shots, "shell-palette.png"));
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${visibleTab}?.querySelector('.statusbar span')?.textContent === '1 row'`, 15_000);

  // A new shortcut for the palette, recorded in the Settings.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.shortcuts') !== null`);
  const paletteKey = `[...document.querySelectorAll('.shortcuts tr')].find(r => r.firstChild?.textContent === 'Command palette').querySelector('.shortcut-key')`;
  await page.eval(`${paletteKey}.scrollIntoView(); ${paletteKey}.click(); ${paletteKey}.focus()`);
  await page.waitFor(`${paletteKey}.textContent === 'Press keys…'`);
  await page.key("j", "KeyJ", 74, CTRL);
  await page.waitFor(`${paletteKey}.textContent === 'Ctrl+J'`);
  await settingsFile(dir, /\[shortcuts\.app\]\s+palette = "Mod\+J"/);
  if (shots) await page.screenshot(join(shots, "shell-shortcuts.png"));

  // Toolbar buttons can be hidden, and shown as icons only.
  await page.eval(`${settingRow("History")}.querySelector('input').click()`);
  await page.eval(`(() => {
    const el = ${settingRow("Buttons")}.querySelector('select');
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, 'text');
    el.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`![...${visibleTab}.querySelectorAll('.toolbar button')].some(b => b.getAttribute('aria-label') === 'History')`);
  // With words, then icons alone (the default) once the setting is reset.
  const scriptText = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Script')?.textContent`;
  await page.waitFor(`${scriptText} === 'Script'`);
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.eval(`(() => {
    const el = ${settingRow("Buttons")}.querySelector('select');
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, 'icons');
    el.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`${scriptText} === ''`);
  // Without words a button keeps its name for screen readers and in the tooltip.
  const runButton = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Run')`;
  await page.waitFor(`${runButton} !== undefined`);
  assert.equal(await page.eval(`${runButton}.title.startsWith('Run')`), true);
  // Run, the main action, keeps its word; the others show their icon alone.
  assert.equal(await page.eval(`${runButton}.textContent`), "Run");
  const scriptButton = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Script')`;
  assert.equal(await page.eval(`${scriptButton}.textContent`), "");
  assert.equal(await page.eval(`!!${scriptButton}.querySelector('svg')`), true);
  const unnamed = await page.eval(
    `JSON.stringify([...${visibleTab}.querySelectorAll('.toolbar button')].filter(b => !b.getAttribute('aria-label') || !b.title).map(b => b.outerHTML.slice(0, 80)))`,
  );
  assert.equal(unnamed, "[]");
  // A toolbar wider than the window scrolls with a visible scrollbar.
  assert.equal(await page.eval(`getComputedStyle(${visibleTab}.querySelector('.toolbar')).scrollbarWidth`), "thin");

  await page.key("j", "KeyJ", 74, CTRL);
  await page.waitFor(`document.querySelector('.palette') !== null`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.palette') === null`);
  await page.key("k", "KeyK", 75, CTRL);
  await page.sleep(300);
  assert.equal(await page.eval(`document.querySelector('.palette')`), null);
}
