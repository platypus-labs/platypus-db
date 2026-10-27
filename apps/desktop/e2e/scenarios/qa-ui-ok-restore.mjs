// QA ui (works, regression guard): after a restart of the page the workspace and the
// appearance come back: the tabs with their text, the pinned one first, the active tab, the
// tabs grouped by connection, the theme, the interface size and the navigator's width.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow } from "../query.mjs";
import { settingsFile } from "../settings.mjs";

const NAME = "QA UI restore";
const tabs = `[...document.querySelectorAll('.tab')]`;
const shown = `document.querySelector('.pane:not([hidden]) .query-tab')`;
const text = `[...(${shown}?.querySelectorAll('.cm-line') ?? [])].map(l => l.textContent).join('\\n')`;
const cssVar = (name) => `getComputedStyle(document.documentElement).getPropertyValue(${JSON.stringify(name)}).trim()`;
const setting = (label) =>
  `[...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_restore.db",
    id: "00000000-0000-4000-8000-00000000e130",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
}

async function write(page, sql) {
  await page.waitFor(`${shown}?.querySelector('.cm-content') != null`);
  await page.eval(`${shown}.querySelector('.cm-content').focus()`);
  await page.type(sql);
  await page.waitFor(`${text} === ${JSON.stringify(sql)}`);
}

export default async function (page, { dir }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${tabs}.length === 1`);
  await write(page, "select 'one'");
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`${tabs}.length === 2`);
  await write(page, "select 'two'");
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`${tabs}.length === 3`);
  await write(page, "select 'three'");
  // Pin the third, then make the second the active one.
  await page.eval(`${tabs}[2].dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 300, clientY: 60 }))`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu [role=menuitem]')].find(b => b.textContent.startsWith('Pin tab')).click()`);
  await page.waitFor(`${tabs}[0].classList.contains('pinned')`);
  await page.eval(`${tabs}[2].click()`);
  await page.waitFor(`${text} === "select 'two'"`);

  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Appearance");
  await page.eval(`[...document.querySelectorAll('.theme-card')].find(c => c.textContent === 'Dracula').click()`);
  await page.settingsTab("Tabs");
  await page.eval(`${setting("Group tabs by connection")}.querySelector('input').click()`);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
  await page.key("=", "Equal", 187, CTRL);
  await page.eval(`document.querySelector('.sidebar-resizer').focus()`);
  await page.key("ArrowRight", "ArrowRight", 39);
  await settingsFile(dir, /theme = "dracula"/);
  await settingsFile(dir, /zoom = "110"/);
  await settingsFile(dir, /group_by_connection = true/);
  await settingsFile(dir, /sidebar_width = 276/);
  // The workspace is saved a moment after the last change.
  await page.waitFor(`(localStorage.getItem('platypus.workspace.v2') ?? '').includes('"pinned":true')`);
  await page.sleep(600);

  await page.eval(`location.reload()`);
  await page.waitFor(`document.querySelectorAll('.tab').length === 3`, 30_000);
  await page.waitFor(`${text} === "select 'two'"`);
  assert.equal(await page.eval(`${tabs}[0].classList.contains('pinned')`), true);
  assert.equal(await page.eval(`document.querySelector('.tab-group')?.textContent`), NAME);
  assert.equal(await page.eval(cssVar("--bg")), "#282a36");
  assert.ok(Math.abs((await page.eval(`devicePixelRatio`)) - 1.1) < 0.01);
  assert.equal(await page.eval(`Math.round(document.querySelector('.sidebar-column').getBoundingClientRect().width)`), 276);
  // The pinned tab kept its text.
  await page.eval(`${tabs}[0].click()`);
  await page.waitFor(`${text} === "select 'three'"`);
}
