// UX review, second pass, the data area, on SQLite: the editor (empty result, error title,
// auto-refresh label, resizable side panel), the explain tab (first real step selected,
// resizable properties), the grid menu (analysis in a submenu, delete last), the object page
// (tab icon, yes/no in Indexes, DDL copy), the table profile (one column of figures) and the
// dashboard (a pencil by the name, a toast on Save).
import assert from "node:assert/strict";
import { CTRL, SHIFT } from "../cdp.mjs";
import { sqliteConnection } from "../query.mjs";
import { openData, openEditor, run, stubClipboard, treeMenu, visibleTab } from "../qa-grid.mjs";

const NAME = "E2E ux data";
const explainTab = `[...document.querySelectorAll('.explain-tab')].find(t => t.style.display !== 'none')`;
const objectTab = `[...document.querySelectorAll('.object-tab:not(.table-profile-tab):not(.dashboard-tab)')].find(t => t.style.display !== 'none')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "uxdata.db",
    id: "00000000-0000-4000-8000-0000000000d2",
    name: NAME,
    sql: `CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT, email TEXT);
           CREATE UNIQUE INDEX parent_email ON parent (email);
           CREATE INDEX parent_name ON parent (name);
           INSERT INTO parent VALUES (1, 'Ada', 'ada@example.com'), (2, 'Bob', 'bob@example.com'), (3, 'Cy', 'cy@example.com');`,
  });
}

const waitToast = (page, text) =>
  page.waitFor(`[...document.querySelectorAll('.toast')].some(t => t.textContent.includes(${JSON.stringify(text)}))`, 10_000);

/** Presses the arrow on a resizer and returns how much the panel's width changed (negative when it shrank). */
async function nudge(page, resizer, panel) {
  const before = await page.eval(`${panel}.getBoundingClientRect().width`);
  await page.eval(`${resizer}.focus()`);
  await page.key("ArrowRight", "ArrowRight", 39);
  await page.waitFor(`Math.abs(${panel}.getBoundingClientRect().width - ${before}) > 1`);
  return Math.round((await page.eval(`${panel}.getBoundingClientRect().width`)) - before);
}

async function editor(page) {
  // An empty result: the column lines of the grid do not run through the message.
  await run(page, "SELECT * FROM parent WHERE id < 0");
  await page.waitFor(`${visibleTab}.querySelector('.result-empty') !== null`);
  assert.notEqual(await page.eval(`getComputedStyle(${visibleTab}.querySelector('.result-empty')).backgroundColor`), "rgba(0, 0, 0, 0)");

  // The error does not start with a bare number.
  await run(page, "SELEC 1");
  await page.waitFor(`${visibleTab}.querySelector('.result-error .state-title') !== null`);
  assert.ok(!/^\d+:/.test(await page.eval(`${visibleTab}.querySelector('.result-error .state-title').textContent`)), "an error code is named, not a bare number");

  // The auto-refresh selector says what it is when it is off.
  assert.equal(await page.eval(`${visibleTab}.querySelector('.auto-refresh select option').textContent`), "Auto-refresh off");

  // The History panel is resizable, and Home brings it back to its default.
  await page.eval(`${visibleTab}.querySelector('.toolbar button[aria-label="History"]').click()`);
  await page.waitFor(`${visibleTab}.parentElement.querySelector('.history') !== null`);
  const panel = `${visibleTab}.parentElement.querySelector('.history')`;
  const resizer = `${visibleTab}.parentElement.querySelector(':scope > .panel-resizer')`;
  assert.equal(Math.abs(await nudge(page, resizer, panel)), 16, "an arrow key moves the panel by 16 px");
  await page.key("Home", "Home", 36);
  await page.waitFor(`Math.round(${panel}.getBoundingClientRect().width) === 340`);
  await page.eval(`${visibleTab}.querySelector('.toolbar button[aria-label="History"]').click()`);
  await page.waitFor(`${visibleTab}.parentElement.querySelector('.history') === null`);
}

async function explain(page) {
  await run(page, "SELECT * FROM parent WHERE name = 'x'");
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("E", "KeyE", 69, CTRL | SHIFT);
  await page.waitFor(`${explainTab}?.querySelector('.explain-node') != null`, 15_000);
  // A step is selected, and it is not the plan's heading.
  await page.waitFor(`${explainTab}.querySelector('.explain-node.selected') != null && ${explainTab}.querySelector('.explain-properties') != null`);
  assert.ok(!/QUERY PLAN/.test(await page.eval(`${explainTab}.querySelector('.explain-properties').textContent`)), "the details are of a step");
  assert.equal(Math.abs(await nudge(page, `${explainTab}.querySelector('.explain-split .panel-resizer')`, `${explainTab}.querySelector('.explain-properties')`)), 16);
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => !t.textContent.startsWith('Explain')).click()`);
  await page.waitFor(`${visibleTab} !== undefined`);
}

async function grid(page) {
  await openData(page, "parent");
  const r = await page.rect(`.query-tab:not([style*="none"]) .grid-scroll`);
  await page.click(r.x + 60, r.y + 52);
  await page.click(r.x + 60, r.y + 52, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  const labels = await page.eval(`[...document.querySelectorAll('.context-menu > button span:first-child')].map(s => s.textContent)`);
  assert.ok(labels.includes("Analyze"), `analysis is a submenu in ${labels}`);
  assert.ok(!labels.includes("Count by value"), "no analysis item on the first level");
  assert.ok(labels.indexOf("Delete row") > labels.indexOf("Add row"), `Delete row after the other row actions in ${labels}`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
}

async function objectPage(page) {
  await page.eval(`[...${visibleTab}.querySelectorAll('.viewer-bar button')].find(b => b.textContent.trim() === 'Structure').click()`);
  await page.waitFor(`${objectTab}?.querySelector('.object-sections') != null`, 15_000);
  // The tab shows the icon of a table, the one of the page's header.
  assert.equal(
    await page.eval(`document.querySelector('.tab.active .tab-icon svg').innerHTML === ${objectTab}.querySelector('.object-icon svg').innerHTML`),
    true,
    "the tab icon is the object's",
  );
  const section = (name) => `[...${objectTab}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith(${JSON.stringify(name)}))`;
  // Columns: the sensitivity choice keeps to its column.
  assert.ok((await page.eval(`${objectTab}.querySelector('select.cell-input').offsetWidth`)) <= 150);
  // Indexes: yes and no in both columns.
  await page.eval(`${section("Indexes")}.click()`);
  await page.waitFor(`${objectTab}.querySelector('.meta-table tbody tr') != null && ${objectTab}.querySelector('.meta-table th')?.textContent === 'Name'`);
  const cells = await page.eval(`[...${objectTab}.querySelectorAll('.meta-table tbody tr')].map(r => [...r.children].map(c => c.textContent))`);
  assert.ok(cells.length >= 2, `indexes: ${JSON.stringify(cells)}`);
  for (const row of cells) assert.ok(["yes", "no"].includes(row[2]) && ["yes", "no"].includes(row[3]), `yes/no in ${JSON.stringify(row)}`);
  assert.equal(cells.find((r) => r[0] === "parent_email")?.[2], "yes");
  assert.equal(cells.find((r) => r[0] === "parent_name")?.[2], "no");
  // DDL: Copy puts it on the clipboard and says so.
  await page.eval(`${section("DDL")}.click()`);
  await page.waitFor(`${objectTab}.querySelector('.object-ddl-bar button') != null`);
  await stubClipboard(page);
  await page.eval(`window.__copied = undefined; ${objectTab}.querySelector('.object-ddl-bar button').click()`);
  await page.waitFor(`window.__copied !== undefined`);
  assert.ok((await page.eval(`window.__copied`)).includes("CREATE TABLE"), "the DDL was copied");
  await waitToast(page, "Copied: DDL");
}

async function profile(page) {
  await treeMenu(page, "parent", "Profile data…");
  await page.waitFor(`document.querySelectorAll('.table-profile-tab .tp-card').length === 3`, 30_000);
  // Every card starts its figures at the same place, numbers or text.
  const xs = await page.eval(`[...document.querySelectorAll('.table-profile-tab .tp-card dd.tp-distinct')].map(e => Math.round(e.getBoundingClientRect().x))`);
  assert.equal(xs.length, 3);
  assert.equal(new Set(xs).size, 1, `figures aligned: ${xs}`);
}

async function dashboard(page) {
  await treeMenu(page, NAME, "New dashboard");
  await page.waitFor(`document.querySelector('.dashboard-tab') != null`);
  const tab = `[...document.querySelectorAll('.dashboard-tab')].find(t => t.style.display !== 'none')`;
  assert.equal(await page.eval(`${tab}.querySelector('.dashboard-title-row svg') != null`), true, "a pencil by the name");
  assert.equal(await page.eval(`${tab}.querySelector('.dashboard-empty button svg') != null`), true, "the empty state's button has the plus icon");
  await page.eval(`${tab}.querySelector('[data-action=save]').click()`);
  await waitToast(page, "Dashboard saved");
}

export default async function (page) {
  await openEditor(page, NAME, null, "parent");
  await editor(page);
  await explain(page);
  await grid(page);
  await objectPage(page);
  await profile(page);
  await dashboard(page);
}
