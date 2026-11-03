// The connection list's own menu and "New connection…" on a connection, the search of the
// database picker and of the Settings, the handle that resizes the navigator, and the glass
// of menus (dialogs are solid).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { settingsFile, settingsLack } from "../settings.mjs";

const NAME = "E2E polish";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const menuItems = `[...document.querySelectorAll('.context-menu [role=menuitem]')].map(b => b.querySelector('span').textContent)`;
const menuItem = (title) =>
  `[...document.querySelectorAll('.context-menu [role=menuitem]')].find(b => b.textContent.startsWith(${JSON.stringify(title)}))`;
const tiles = `[...document.querySelectorAll('.driver-tile')].map(b => b.textContent)`;
// The settings in view: a panel's own rows (Sync's) stay in the DOM while the search hides them.
const shownSettings = `[...document.querySelectorAll('.modal.settings .setting')].filter(s => s.offsetParent !== null).map(s => s.querySelector('span')?.firstChild?.textContent)`;
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
  // Where earlier builds kept the navigator's width and collapse: still read.
  writeFileSync(join(dir, "settings.toml"), "[ui]\nsidebar_width = 300\nsidebar_collapsed = true\n");
}

export default async function (page, { dir, shots }) {
  await page.waitFor(`${row(NAME)} !== undefined`);

  // The keys of earlier builds: the navigator starts hidden, at their width. Shown again, the
  // old key goes and no new one is written (shown is the default).
  await page.waitFor(`document.querySelector('.sidebar-column').hidden`);
  await page.key("b", "KeyB", 66, CTRL);
  await page.waitFor(`!document.querySelector('.sidebar-column').hidden`);
  await settingsLack(dir, /sidebar_collapsed/);
  assert.equal(await page.eval(sidebarWidth), 300);

  // Below the rows, the list's own menu: a new connection, the import and export, testing them
  // all, a plan.
  await page.eval(`document.querySelector('.tree').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 150, clientY: 400 }))`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  assert.deepEqual(await page.eval(menuItems), ["New connection…", "Import connections…", "Export connections…", "Test connections", "Open query plan…"]);
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
  // With several matches, Enter takes the first: "sql" opens the PostgreSQL form.
  await page.eval(`document.querySelector('button[title="New connection"]').click()`);
  await page.waitFor(`document.activeElement === document.querySelector('.driver-picker .modal-search input')`);
  // PostgreSQL is the first tile of the whole list too: the search must have left fewer.
  const all = await page.eval(`${tiles}.length`);
  await page.eval(setInput(".driver-picker .modal-search input", "sql"));
  await page.waitFor(`${tiles}.length < ${all} && ${tiles}.length > 1 && ${tiles}[0] === 'PostgreSQL'`);
  await page.key("Enter", "Enter", 13);
  // The form names its database by the logo in its header.
  const driver = `document.querySelector('.connection-dialog .dialog-header .driver-icon')?.getAttribute('aria-label')`;
  await page.waitFor(`document.querySelector('.modal.driver-picker') === null && ${driver} === 'PostgreSQL'`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.modal') === null`);

  // A connection's menu starts with a new connection.
  await page.eval(`${row(NAME)}.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 120, clientY: 120 }))`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  assert.equal(await page.eval(`${menuItems}[0]`), "New connection…");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);

  // The handle between the navigator and the work: dragged wider, saved under the new key
  // (the old one goes), and reset.
  const before = await page.eval(sidebarWidth);
  assert.equal(before, 300);
  const handle = await page.rect(".sidebar-resizer");
  const x = handle.x + handle.w / 2;
  const y = handle.y + 200;
  await page.mouse("mousePressed", x, y);
  await page.mouse("mouseMoved", x + 60, y);
  await page.mouse("mouseMoved", x + 100, y);
  await page.mouse("mouseReleased", x + 100, y);
  await page.waitFor(`${sidebarWidth} === ${before + 100}`);
  await settingsFile(dir, new RegExp(`\\[layout\\][^[]*sidebar_width = ${before + 100}`));
  await settingsLack(dir, /\[ui\][^[]*sidebar_width/);
  if (shots) await page.screenshot(join(shots, "polish-resized.png"));
  await page.eval(`document.querySelector('.sidebar-resizer').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`${sidebarWidth} === 260`);
  // Back to the default, the width is no longer stored.
  await settingsLack(dir, /sidebar_width/);
  // A click without moving saves nothing: the writes of the width are counted, and the arrow
  // key after the click, which saves, is the only one. Tauri's invoke cannot be replaced; the
  // commands reach the app through fetch.
  await page.eval(`(() => {
    const fetch = window.fetch.bind(window);
    window.__widthWrites = 0;
    window.fetch = (url, options) => {
      if (String(url).endsWith('/set_setting') && String(options?.body ?? '').includes('sidebar_width')) window.__widthWrites++;
      return fetch(url, options);
    };
  })()`);
  const reset = await page.rect(".sidebar-resizer");
  const [cx, cy] = [reset.x + reset.w / 2, reset.y + 200];
  assert.equal(await page.eval(`document.elementFromPoint(${cx}, ${cy})?.classList.contains('sidebar-resizer')`), true);
  await page.click(cx, cy);
  await page.eval(`document.querySelector('.sidebar-resizer').focus()`);
  await page.key("ArrowRight", "ArrowRight", 39);
  await page.waitFor(`window.__widthWrites === 1 && ${sidebarWidth} === 276`);
  await page.sleep(300);
  assert.equal(await page.eval(`window.__widthWrites`), 1);

  // The Settings' search keeps the matching settings, and their sections only.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.waitFor(`document.activeElement === document.querySelector('.modal.settings .modal-search input')`);
  // Solid: the glass behind a dialog this large made its scrolling lag (s-fix-settings-scroll).
  assert.equal(await page.eval(`getComputedStyle(document.querySelector('.modal.settings')).backdropFilter`), "none");
  // A tab other than the first, to find it again after the search.
  await page.settingsTab("Toolbar");
  // The search matches at the start of a word: "tab" finds the tabs, not "databases".
  await page.eval(setInput(".modal.settings .modal-search input", "tab"));
  await page.waitFor(`${shownSettings}.includes('Group tabs by connection')`);
  assert.equal(await page.eval(`${shownSettings}.includes('Language')`), false);
  await page.eval(setInput(".modal.settings .modal-search input", "language"));
  // The setting, and those of the sub-tab named after it ("Language and text").
  await page.waitFor(`${shownSettings}.length === 3`);
  assert.deepEqual(await page.eval(shownSettings), ["Language", "SQL editor font size", "Result grid font size"]);
  assert.equal(await page.eval(`document.querySelectorAll('.settings-nav button').length`), 1);
  // The matches show grouped under their section, in place of the tabs.
  assert.deepEqual(await page.eval(`[...document.querySelectorAll('.settings-results > section:not([hidden]) > h3')].map(h => h.textContent)`), ["Appearance"]);
  // The settings' own tabs; a panel's tabs (the agents') stay in their panel.
  assert.equal(await page.eval(`document.querySelector('.modal.settings .settings-nav[role="tablist"]') === null`), true);
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
  await page.eval(`document.querySelector('.modal.settings .modal-search input').focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.settings-none') === null && ${shownSettings}.length > 1`);
  // With the search empty, the tabs are back, on the section shown before.
  assert.equal(await page.eval(`document.querySelector('.modal.settings [role="tab"][aria-selected="true"]')?.dataset.section`), "Toolbar");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}
