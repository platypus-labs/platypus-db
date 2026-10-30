// The shell's second round: the tab menu (pin, close others), tabs grouped by connection,
// the split editor, a table dragged from the navigator into the editor, the zoom keys, the
// Activity log, the dialogs' and the tree's keyboard support, and the Italian interface.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, SHIFT } from "../cdp.mjs";
import { settingsFile, settingsLack } from "../settings.mjs";

const NAME = "E2E tabs";
const tabs = `[...document.querySelectorAll('.tab')]`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const shownEditors = `[...document.querySelectorAll('.pane:not([hidden]) .query-tab')]`;
const editorText = (editor) => `[...${editor}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const menuItem = (title) =>
  `[...document.querySelectorAll('.context-menu [role=menuitem]')].find(b => b.textContent.startsWith(${JSON.stringify(title)}))`;

export function setup({ dir }) {
  const path = join(dir, "tabs.db");
  const db = new DatabaseSync(path);
  db.exec("CREATE TABLE orders (id INTEGER PRIMARY KEY, total REAL); INSERT INTO orders VALUES (1, 9.5);");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000031"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

/** Opens the context menu of the tab at `index`. */
async function tabMenu(page, index) {
  await page.eval(`${tabs}[${index}].dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 200, clientY: 60 }))`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
}

/** Sets a select of the Settings dialog, found by its row's label. */
const pick = (label, value) => `(() => {
  const row = [...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === ${JSON.stringify(label)});
  const el = row.querySelector('select');
  Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('change', { bubbles: true }));
})()`;

export default async function (page, { dir, shots }) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${tabs}.length === 1`);
  await page.key("t", "KeyT", 84, CTRL);
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`${tabs}.length === 3`);

  // A middle click closes the tab under it. With tabs that overflow the bar, Chromium starts
  // its autoscroll on the press and swallows the click, unless the press is cancelled.
  await page.eval(`document.head.insertAdjacentHTML('beforeend', '<style id="e2e-wide">.tab { min-width: 600px }</style>')`);
  await page.waitFor(`document.querySelector('.tabs').scrollWidth > document.querySelector('.tabs').clientWidth`);
  await page.eval(`document.querySelector('.tabs').scrollLeft = 0`);
  await page.clickSel(".tab:nth-child(1)", { button: "middle" });
  await page.waitFor(`${tabs}.length === 2`);
  await page.eval(`document.getElementById('e2e-wide').remove()`);
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`${tabs}.length === 3`);

  // The sidebar: its handle resizes it, a double click restores the width, Ctrl+B hides it.
  const sidebarWidth = `Math.round(document.querySelector('.sidebar-column').getBoundingClientRect().width)`;
  assert.equal(await page.eval(sidebarWidth), 260);
  const handle = await page.rect(".sidebar-resizer");
  const [hx, hy] = [handle.x + handle.w / 2, handle.y + handle.h / 2];
  await page.mouse("mousePressed", hx, hy);
  await page.mouse("mouseMoved", hx + 40, hy);
  await page.mouse("mouseMoved", hx + 60, hy);
  await page.mouse("mouseReleased", hx + 60, hy);
  await page.waitFor(`${sidebarWidth} === 320`);
  await settingsFile(dir, /\[layout\][^[]*sidebar_width = 320/);
  await page.eval(`document.querySelector('.sidebar-resizer').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`${sidebarWidth} === 260`);
  await settingsLack(dir, /sidebar_width/);
  await page.key("b", "KeyB", 66, CTRL);
  await page.waitFor(`document.querySelector('.sidebar-column').hidden`);
  await settingsFile(dir, /\[layout\][^[]*sidebar_collapsed = true/);
  await page.eval(`document.querySelector('.topbar button[aria-label="Show or hide the navigator"]').click()`);
  await page.waitFor(`!document.querySelector('.sidebar-column').hidden`);
  await settingsLack(dir, /sidebar_collapsed/);

  // The tab bar is a tab list; its menu works from the keyboard and pins a tab.
  assert.equal(await page.eval(`document.querySelector('.tabs').getAttribute('role')`), "tablist");
  assert.equal(await page.eval(`${tabs}.filter(t => t.getAttribute('aria-selected') === 'true').length`), 1);
  await tabMenu(page, 0);
  await page.waitFor(`document.activeElement?.getAttribute('role') === 'menuitem'`);
  await page.eval(`${menuItem("Pin tab")}.click()`);
  await page.waitFor(`${tabs}[0].classList.contains('pinned')`);
  assert.equal(await page.eval(`${tabs}[0].querySelector('button') === null`), true);

  // "Close others" leaves the pinned tab open.
  await tabMenu(page, 2);
  await page.eval(`${menuItem("Close others")}.click()`);
  await page.waitFor(`${tabs}.length === 2`);
  assert.equal(await page.eval(`${tabs}[0].classList.contains('pinned')`), true);

  // Split: the active tab and the next one side by side; a click in a pane makes it active.
  await page.key("D", "KeyD", 68, CTRL | SHIFT);
  await page.waitFor(`document.querySelector('.panes.split') !== null && ${shownEditors}.length === 2`);
  await page.eval(`${shownEditors}[0].closest('.pane').dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))`);
  await page.waitFor(`${shownEditors}[0].closest('.pane').classList.contains('focused')`);
  if (shots) await page.screenshot(join(shots, "shell-split.png"));
  await page.key("D", "KeyD", 68, CTRL | SHIFT);
  await page.waitFor(`document.querySelector('.panes.split') === null && ${shownEditors}.length === 1`);

  // A table dragged into an empty editor writes a query of it; into text, its name.
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("orders")} !== undefined || ${row("main")} !== undefined`, 15_000);
  if (await page.eval(`${row("orders")} === undefined`)) {
    await page.eval(`${row("main")}.querySelector('.twisty').click()`);
    await page.waitFor(`${row("orders")} !== undefined`, 15_000);
  }
  // Real pointer input: press on the row, move with the button held, release in the editor.
  const editor = `${shownEditors}[0]`;
  const content = `${editor}.querySelector('.cm-content')`;
  const drop = async (options = {}) => {
    const from = await page.rectOf(row("orders"));
    const to = await page.rectOf(content);
    await page.drag(from.x + 40, from.y + from.h / 2, to.x + 40, to.y + 8, options);
  };
  let ghost = null;
  await drop({ before: async () => (ghost = await page.eval(`document.querySelector('.drag-ghost')?.textContent`)) });
  assert.match(ghost ?? "", /orders/);
  await page.waitFor(`${editorText(editor)}.startsWith('SELECT')`);
  assert.match(await page.eval(editorText(editor)), /FROM .*orders/);
  assert.equal(await page.eval(`document.querySelector('.drag-ghost') === null`), true);
  // Into text: the name, where it is dropped.
  await drop();
  await page.waitFor(`(${editorText(editor)}.match(/orders/g) ?? []).length === 2`);
  // With Shift held: a query of it, even into text.
  await drop({ modifiers: SHIFT });
  await page.waitFor(`(${editorText(editor)}.match(/FROM/g) ?? []).length === 2`);
  // Escape cancels: nothing is dropped.
  const text = await page.eval(editorText(editor));
  await drop({ before: () => page.key("Escape", "Escape", 27) });
  await page.sleep(300);
  assert.equal(await page.eval(editorText(editor)), text);
  // Released outside any editor: nothing either, and the row is not dragged anywhere.
  const r = await page.rectOf(row("orders"));
  await page.drag(r.x + 40, r.y + r.h / 2, r.x + 40, r.y + r.h / 2 + 60);
  await page.sleep(300);
  assert.equal(await page.eval(editorText(editor)), text);

  // The tree from the keyboard: the arrows move the selection and the focus.
  await page.eval(`${row(NAME)}.focus()`);
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.waitFor(`document.activeElement?.getAttribute('role') === 'treeitem' && document.activeElement !== ${row(NAME)}`);
  assert.equal(await page.eval(`document.activeElement.classList.contains('selected')`), true);

  // Ctrl+= makes the interface bigger, Ctrl+0 brings it back.
  await page.key("=", "Equal", 187, CTRL);
  await settingsFile(dir, /zoom = "110"/);
  await page.key("0", "Digit0", 48, CTRL);
  await settingsLack(dir, /zoom = "110"/);

  // The Activity log has the connection that opened, and dialogs carry their role and name.
  await page.eval(`document.querySelector('.activity-button').click()`);
  await page.waitFor(`document.querySelector('.modal.activity') !== null`);
  assert.match(await page.eval(`document.querySelector('.activity-list')?.textContent ?? ''`), /Connect to E2E tabs/);
  if (shots) await page.screenshot(join(shots, "shell-activity.png"));
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.activity') === null`);

  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.waitFor(`document.querySelector('.modal.settings').getAttribute('role') === 'dialog'`);
  const named = `document.getElementById(document.querySelector('.modal.settings').getAttribute('aria-labelledby'))?.textContent`;
  assert.equal(await page.eval(named), "Settings");

  // Tabs grouped by connection: the connection's name before its tabs.
  await page.settingsTab("Tabs");
  await page.eval(`[...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === 'Group tabs by connection').querySelector('input').click()`);
  await page.waitFor(`document.querySelector('.tab-group')?.textContent === ${JSON.stringify(NAME)}`);

  // Italian: the interface changes at once, and back.
  await page.settingsTab("Appearance");
  await page.eval(pick("Language", "it"));
  await page.waitFor(`document.querySelector('.modal.settings h2')?.textContent === 'Impostazioni'`);
  assert.equal(await page.eval(`document.querySelector('.topbar-search span').textContent`), "Cerca tabelle, query, comandi");
  assert.equal(await page.eval(`document.documentElement.lang`), "it");
  if (shots) await page.screenshot(join(shots, "shell-italian.png"));
  await settingsFile(dir, /language = "it"/);
  const lingua = `[...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === 'Lingua')`;
  await page.eval(`(() => {
    const el = ${lingua}.querySelector('select');
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, 'en');
    el.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
  await page.waitFor(`document.querySelector('.modal.settings h2')?.textContent === 'Settings'`);
}
