// The connection list's own menu and "New connection…" on a connection, the search of the
// database picker and of the Settings, the handle that resizes the navigator, and the glass
// of menus and dialogs.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E polish";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const menuItems = `[...document.querySelectorAll('.context-menu [role=menuitem]')].map(b => b.querySelector('span').textContent)`;
const menuItem = (title) =>
  `[...document.querySelectorAll('.context-menu [role=menuitem]')].find(b => b.textContent.startsWith(${JSON.stringify(title)}))`;
const tiles = `[...document.querySelectorAll('.driver-tile')].map(b => b.textContent)`;
const shownSettings = `[...document.querySelectorAll('.modal.settings .setting')].map(s => s.querySelector('span')?.firstChild?.textContent)`;
const sidebarWidth = `Math.round(document.querySelector('.sidebar-column').getBoundingClientRect().width)`;

/** Types into an input as the user would, for React's `onChange`. */
const setInput = (selector, value) => `(() => {
  const el = document.querySelector(${JSON.stringify(selector)});
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('input', { bubbles: true }));
})()`;

export function setup({ dir }) {
  const path = join(dir, "polish.db");
  const db = new DatabaseSync(path);
  db.exec("CREATE TABLE items (id INTEGER PRIMARY KEY);");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000041"
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

export default async function (page, { dir, shots }) {
  await page.waitFor(`${row(NAME)} !== undefined`);

  // Below the rows, the list's own menu: a new connection, or the import.
  await page.eval(`document.querySelector('.tree').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 150, clientY: 400 }))`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  assert.deepEqual(await page.eval(menuItems), ["New connection…", "Import connections…"]);
  const glass = await page.eval(`getComputedStyle(document.querySelector('.context-menu')).backdropFilter`);
  assert.match(glass, /blur/);
  await page.eval(`${menuItem("New connection…")}.click()`);

  // The database picker: four columns, and a search that keeps the matching ones.
  await page.waitFor(`document.querySelector('.modal.driver-picker') !== null`);
  assert.equal(await page.eval(`getComputedStyle(document.querySelector('.driver-grid')).gridTemplateColumns.split(' ').length`), 4);
  await page.waitFor(`document.activeElement === document.querySelector('.driver-picker .modal-search input')`);
  await page.eval(setInput(".driver-picker .modal-search input", "duck"));
  await page.waitFor(`${tiles}.length === 1`);
  assert.deepEqual(await page.eval(tiles), ["DuckDB"]);
  // The protocol finds the compatible servers too.
  await page.eval(setInput(".driver-picker .modal-search input", "postgres"));
  await page.waitFor(`${tiles}.includes('CockroachDB')`);
  assert.equal(await page.eval(`${tiles}.includes('PostgreSQL') && !${tiles}.includes('MySQL')`), true);
  await page.eval(setInput(".driver-picker .modal-search input", "zzz"));
  await page.waitFor(`document.querySelector('.driver-none') !== null && ${tiles}.length === 0`);
  if (shots) await page.screenshot(join(shots, "polish-picker.png"));
  // Escape clears the search first, then closes.
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${tiles}.includes('PostgreSQL') && ${tiles}.includes('Neon')`);
  assert.equal(await page.eval(`document.querySelector('.modal.driver-picker') !== null`), true);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.driver-picker') === null`);

  // A connection's menu starts with a new connection.
  await page.eval(`${row(NAME)}.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 120, clientY: 120 }))`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  assert.equal(await page.eval(`${menuItems}[0]`), "New connection…");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);

  // The handle between the navigator and the work: dragged wider, saved, and reset.
  const before = await page.eval(sidebarWidth);
  const handle = await page.rect(".sidebar-resizer");
  const x = handle.x + handle.w / 2;
  const y = handle.y + 200;
  await page.mouse("mousePressed", x, y);
  await page.mouse("mouseMoved", x + 60, y);
  await page.mouse("mouseMoved", x + 100, y);
  await page.mouse("mouseReleased", x + 100, y);
  await page.waitFor(`${sidebarWidth} === ${before + 100}`);
  await settingsFile(dir, new RegExp(`sidebar_width = ${before + 100}`));
  if (shots) await page.screenshot(join(shots, "polish-resized.png"));
  await page.eval(`document.querySelector('.sidebar-resizer').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`${sidebarWidth} === ${before}`);
  // A click without moving saves nothing.
  await page.click(x, y);
  await settingsFile(dir, /^(?![\s\S]*sidebar_width)/);

  // The Settings' search keeps the matching settings, and their sections only.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.waitFor(`document.activeElement === document.querySelector('.modal.settings .modal-search input')`);
  assert.match(await page.eval(`getComputedStyle(document.querySelector('.modal.settings')).backdropFilter`), /blur/);
  await page.eval(setInput(".modal.settings .modal-search input", "language"));
  await page.waitFor(`${shownSettings}.length === 1`);
  assert.deepEqual(await page.eval(shownSettings), ["Language"]);
  assert.equal(await page.eval(`document.querySelectorAll('.settings-nav button').length`), 1);
  // An action's name finds the shortcuts, filtered to it.
  // Panels the search leaves out are hidden.
  const shortcutsShown = `document.querySelector('.modal.settings .shortcuts')?.closest('section').hidden === false`;
  assert.equal(await page.eval(shortcutsShown), false);
  await page.eval(setInput(".modal.settings .modal-search input", "reopen closed tab"));
  await page.waitFor(shortcutsShown);
  assert.equal(await page.eval(`${shownSettings}.length`), 0);
  assert.equal(await page.eval(`document.querySelectorAll('.modal.settings .shortcuts tbody tr:not(.shortcuts-scope)').length`), 1);
  await page.eval(setInput(".modal.settings .modal-search input", "zzzz"));
  await page.waitFor(`document.querySelector('.settings-none') !== null`);
  if (shots) await page.screenshot(join(shots, "polish-settings.png"));
  // Escape clears the search first, then closes.
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.settings-none') === null && ${shownSettings}.length > 1`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}
