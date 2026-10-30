// Settings › Appearance › Layout: the navigator on the right, the results beside the editor,
// the tab bar at the bottom and the side panels on the left. Each choice applies at once (the
// root's attribute and where the parts are), is written to settings.toml, is read at start,
// and goes with Reset; the handles that resize the navigator, the editor and the side panels
// follow the pointer on either side. The settings' search finds the choices by name.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { runQuery, sqliteConnection, treeRow, visibleTab } from "../query.mjs";
import { settingsFile, settingsLack } from "../settings.mjs";

const NAME = "E2E layout";
const KEYS = ["navigator_side", "results_position", "tab_bar_position", "side_panels_side"];

const setting = (key) => `document.querySelector('.modal.settings .setting[data-key="layout.${key}"]')`;
const choice = (key, value) => `${setting(key)}?.querySelector('[role=radio][data-value="${value}"]')`;
const resetOf = (key) => `[...${setting(key)}.querySelectorAll('button')].find(b => b.textContent === 'Reset')`;
const subTab = (label) => `document.querySelector('.modal.settings [role=tab][data-subtab=${JSON.stringify(label)}]')`;
const attr = (name) => `document.documentElement.dataset.${name}`;
/** The box of the element `expr` evaluates to, as plain numbers. */
const box = (expr) => `(() => { const r = (${expr}).getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height }; })()`;
const editor = `${visibleTab}.querySelector('.query-main > .editor')`;
const results = `${visibleTab}.querySelector('.query-main > .results')`;
const splitter = `${visibleTab}.querySelector('.query-main > .splitter')`;
const width = (expr) => `Math.round((${expr}).getBoundingClientRect().width)`;

/** Types into an input as the user would, for React's `onChange`. */
const setInput = (selector, value) => `(() => {
  const el = document.querySelector(${JSON.stringify(selector)});
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('input', { bubbles: true }));
})()`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "layout.db",
    id: "00000000-0000-4000-8000-000000000071",
    name: NAME,
    sql: "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO items VALUES (1, 'a'), (2, 'b'), (3, 'c');",
  });
  // Saved by an earlier run: the tab bar at the bottom. No animation, so that the boxes measured
  // are where the parts stay.
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nanimations = "off"\n\n[layout]\ntab_bar_position = "bottom"\n');
}

async function openSettingsLayout(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Appearance");
  await page.waitFor(`${subTab("Layout")} !== null`);
  await page.eval(`${subTab("Layout")}.click()`);
  await page.waitFor(`${subTab("Layout")}.getAttribute('aria-selected') === 'true' && ${setting("navigator_side")}?.offsetParent != null`);
}

async function closeSettings(page) {
  await page.eval(`[...document.querySelectorAll('.modal.settings .actions button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

/** Presses on the middle of the element `expr` evaluates to (at `dy` below its top for a tall
 *  one), moves by `dx` and releases. */
async function drag(page, expr, dx, dy = null) {
  const r = await page.eval(box(expr));
  const x = r.left + r.width / 2;
  const y = dy === null ? r.top + r.height / 2 : r.top + dy;
  await page.mouse("mousePressed", x, y);
  await page.mouse("mouseMoved", x + dx / 2, y);
  await page.mouse("mouseMoved", x + dx, y);
  await page.mouse("mouseReleased", x + dx, y);
}

export default async function (page, { dir, shots }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  // The defaults on the root, but the tab bar read from the file.
  await page.waitFor(`${attr("tabbar")} === 'bottom'`);
  assert.equal(await page.eval(attr("navigator")), "left");
  assert.equal(await page.eval(attr("results")), "below");
  assert.equal(await page.eval(attr("panels")), "right");
  assert.equal(await page.eval(attr("motion")), "none");

  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  await runQuery(page, "SELECT * FROM items");

  // The tab bar under the tab's pane, at the bottom of the sheet.
  const tabsBottom = await page.eval(box(`document.querySelector('.workspace > .tabs')`));
  const panes = await page.eval(box(`document.querySelector('.workspace > .panes')`));
  assert.ok(tabsBottom.top >= panes.bottom - 1, `tab bar ${tabsBottom.top} below the panes ${panes.bottom}`);
  const sheet = await page.eval(box(`document.querySelector('.workspace')`));
  assert.ok(sheet.bottom - tabsBottom.bottom < 3, "the tab bar ends the sheet");

  // The Layout sub-tab: four choices of two pictures each, the saved one checked; the dialog
  // fits the window.
  await openSettingsLayout(page);
  for (const key of KEYS) assert.equal(await page.eval(`${setting(key)}.querySelectorAll('[role=radio]').length`), 2, key);
  assert.equal(await page.eval(`${choice("tab_bar_position", "bottom")}.getAttribute('aria-checked')`), "true");
  assert.equal(await page.eval(`${choice("navigator_side", "left")}.getAttribute('aria-checked')`), "true");
  const dialog = await page.eval(box(`document.querySelector('.modal.settings')`));
  assert.ok(dialog.top >= 0 && dialog.bottom <= (await page.eval("window.innerHeight")), "the dialog fits the window's height");
  // The two columns of choices sit side by side.
  const nav = await page.eval(box(setting("navigator_side")));
  const res = await page.eval(box(setting("results_position")));
  assert.ok(Math.abs(nav.top - res.top) < 2 && res.left > nav.right - 1, "the choices in two columns");
  if (shots) await page.screenshot(join(shots, "layout-settings.png"));

  // Reset puts the tab bar back on top, and takes the key out of the file.
  await page.eval(`${resetOf("tab_bar_position")}.click()`);
  await page.waitFor(`${attr("tabbar")} === 'top'`);
  await settingsLack(dir, /tab_bar_position/);
  assert.equal(await page.eval(`${choice("tab_bar_position", "top")}.getAttribute('aria-checked')`), "true");
  const tabsTop = await page.eval(box(`document.querySelector('.workspace > .tabs')`));
  const panesBelow = await page.eval(box(`document.querySelector('.workspace > .panes')`));
  assert.ok(tabsTop.bottom <= panesBelow.top + 1, "the tab bar above the panes");

  // The navigator on the right of the sheet.
  await page.eval(`${choice("navigator_side", "right")}.click()`);
  await page.waitFor(`${attr("navigator")} === 'right'`);
  await settingsFile(dir, /\[layout\][^[]*navigator_side = "right"/);
  await page.waitFor(
    `document.querySelector('.sidebar-column').getBoundingClientRect().left >= document.querySelector('.workspace').getBoundingClientRect().right - 1`,
  );
  // The project rail stays on the left.
  assert.ok(
    (await page.eval(box(`document.querySelector('.project-rail')`))).right <= (await page.eval(box(`document.querySelector('.workspace')`))).left + 1,
  );

  // The side panels on the left.
  await page.eval(`${choice("side_panels_side", "left")}.click()`);
  await page.waitFor(`${attr("panels")} === 'left'`);
  await settingsFile(dir, /side_panels_side = "left"/);
  if (shots) await page.screenshot(join(shots, "layout-mirrored.png"));
  await closeSettings(page);

  // The navigator's handle is on its left edge: dragged left, the navigator widens; arrows
  // too; a double-click goes back to the default.
  const column = `document.querySelector('.sidebar-column')`;
  const handle = await page.eval(box(`document.querySelector('.sidebar-resizer')`));
  const side = await page.eval(box(column));
  assert.ok(Math.abs(handle.left + handle.width / 2 - side.left) < 3, `handle at ${handle.left}, navigator at ${side.left}`);
  assert.equal(await page.eval(width(column)), 260);
  await drag(page, `document.querySelector('.sidebar-resizer')`, -100, 200);
  await page.waitFor(`${width(column)} === 360`);
  await settingsFile(dir, /sidebar_width = 360/);
  await page.eval(`document.querySelector('.sidebar-resizer').focus()`);
  await page.key("ArrowLeft", "ArrowLeft", 37);
  await page.waitFor(`${width(column)} === 376`);
  await page.eval(`document.querySelector('.sidebar-resizer').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`${width(column)} === 260`);
  await settingsLack(dir, /sidebar_width/);

  // The history beside the editor, on its left.
  const historyButton = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'History')`;
  await page.eval(`${historyButton}.click()`);
  await page.waitFor(`${visibleTab}.querySelector('aside.history') != null`);
  await page.waitFor(
    `${visibleTab}.querySelector('aside.history').getBoundingClientRect().right <= ${visibleTab}.querySelector('.query-main').getBoundingClientRect().left + 1`,
  );
  await page.eval(`${historyButton}.click()`);
  await page.waitFor(`${visibleTab}.querySelector('aside.history') === null`);

  // The value panel on the left of the grid; its handle on the panel's right edge: dragged
  // right, the panel widens.
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("Home", "Home", 36, CTRL);
  await page.key("F7", "F7", 118);
  const panel = `${visibleTab}.querySelector('.result-grid-body > .value-panel')`;
  await page.waitFor(`${panel} != null`);
  await page.waitFor(`${panel}.getBoundingClientRect().right <= ${visibleTab}.querySelector('.result-grid-main').getBoundingClientRect().left + 1`);
  const before = await page.eval(width(panel));
  await drag(page, `${visibleTab}.querySelector('.result-grid-body > .panel-resizer')`, 40);
  await page.waitFor(`Math.abs(${width(panel)} - ${before + 40}) <= 1`);
  await settingsFile(dir, /side_panel_width = \d+/);
  await page.eval(`${visibleTab}.querySelector('.result-grid-body > .panel-resizer').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await settingsLack(dir, /side_panel_width/);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("F7", "F7", 118);
  await page.waitFor(`${panel} === null`);

  // The results beside the editor: side by side, the toolbar and the status bar across both.
  await openSettingsLayout(page);
  await page.eval(`${choice("results_position", "beside")}.click()`);
  await page.waitFor(`${attr("results")} === 'beside'`);
  await settingsFile(dir, /results_position = "beside"/);
  await closeSettings(page);
  await page.waitFor(`${results}.getBoundingClientRect().left >= ${editor}.getBoundingClientRect().right`);
  const ed = await page.eval(box(editor));
  const rs = await page.eval(box(results));
  assert.ok(Math.abs(ed.top - rs.top) < 2, `editor top ${ed.top}, results top ${rs.top}`);
  const toolbar = await page.eval(box(`${visibleTab}.querySelector('.query-main > .toolbar')`));
  const statusbar = await page.eval(box(`${visibleTab}.querySelector('.query-main > .statusbar')`));
  assert.ok(toolbar.left <= ed.left + 1 && toolbar.right >= rs.right - 1, "the toolbar spans the editor and the results");
  assert.ok(statusbar.top >= rs.bottom - 1 && statusbar.right >= rs.right - 1, "the status bar under both");
  assert.equal(await page.eval(`${splitter}.getAttribute('aria-orientation')`), "vertical");
  if (shots) await page.screenshot(join(shots, "layout-beside.png"));
  // The handle between them sets the editor's width.
  const edWidth = await page.eval(width(editor));
  await drag(page, splitter, 40);
  await page.waitFor(`Math.abs(${width(editor)} - ${edWidth + 40}) <= 1`);
  await settingsFile(dir, /editor_width = \d+/);
  await page.eval(`${splitter}.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await settingsLack(dir, /editor_width/);
  // The editor still runs, and its results still show.
  await runQuery(page, "SELECT * FROM items WHERE id < 3");
  assert.equal(await page.eval(`${results}.querySelector('.grid-scroll') != null`), true);

  // The search finds the choices by name, and by the name of their sub-tab.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.eval(setInput(".modal.settings .modal-search input", "tab bar"));
  await page.waitFor(`${setting("tab_bar_position")} !== null`);
  assert.equal(await page.eval(`${setting("navigator_side")} === null`), true);
  await page.eval(setInput(".modal.settings .modal-search input", "layout"));
  await page.waitFor(`${KEYS.map((k) => `${setting(k)} !== null`).join(" && ")}`);
  await page.eval(setInput(".modal.settings .modal-search input", ""));
  await page.waitFor(`document.querySelector('.modal.settings [role=tab][data-section]') !== null`);
  await closeSettings(page);

  // Reset brings every part back where it was, and the file keeps none of the keys.
  await openSettingsLayout(page);
  for (const key of ["navigator_side", "results_position", "side_panels_side"]) {
    await page.eval(`${resetOf(key)}.click()`);
  }
  await page.waitFor(`${attr("navigator")} === 'left' && ${attr("results")} === 'below' && ${attr("panels")} === 'right'`);
  await settingsLack(dir, /navigator_side|results_position|tab_bar_position|side_panels_side/);
  await closeSettings(page);
  await page.waitFor(
    `document.querySelector('.sidebar-column').getBoundingClientRect().right <= document.querySelector('.workspace').getBoundingClientRect().left + 1`,
  );
  await page.waitFor(`${results}.getBoundingClientRect().top >= ${editor}.getBoundingClientRect().bottom`);
  assert.equal(await page.eval(`${splitter}.getAttribute('aria-orientation')`), "horizontal");
}
