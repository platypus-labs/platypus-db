// Small UX fixes (group T) on SQLite:
// - the start page is centered in the workspace;
// - the connection dialog titles the page that is shown, and the pages that hold settings carry
//   a dot with an explanation;
// - the command palette's search input keeps its full height with the long list under it;
// - the empty list of transfer profiles says where one is created and has a button to go there;
// - the dashboard's "Chart settings" item shows the Icon component's mark when it is on.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { dialog, editConnection } from "../conn-helpers.mjs";
import { setField } from "../plans.mjs";
import { closeProfiles, openProfilesOf, profilesDialog } from "../profiles-helpers.mjs";
import { sqliteConnection } from "../query.mjs";
import { treeMenu } from "../qa-grid.mjs";

const NAME = "E2E ux small";
const ID = "00000000-0000-4000-8000-0000000000c1";

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "ux.db",
    id: ID,
    name: NAME,
    sql: `CREATE TABLE sales (id INTEGER PRIMARY KEY, region TEXT, amount INTEGER);
           INSERT INTO sales VALUES (1, 'north', 10), (2, 'south', 5), (3, 'east', 3);`,
  });
  const folder = join(dir, "scripts", "all");
  mkdirSync(folder, { recursive: true });
  writeFileSync(join(folder, "regions.sql"), "SELECT region, sum(amount) AS total FROM sales GROUP BY region ORDER BY region");
  writeFileSync(join(folder, ".links.json"), JSON.stringify({ "regions.sql": ID }));
}

/** The center of the element's box, as {x, y}. */
const centerOf = (selector) =>
  `(() => { const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2, w: r.width, h: r.height }; })()`;

async function startPage(page) {
  await page.waitFor(`document.querySelector('.welcome') != null`);
  const welcome = await page.eval(centerOf(".welcome"));
  const space = await page.eval(centerOf(".workspace"));
  assert.ok(Math.abs(welcome.x - space.x) <= 40, `the start page is centered across: ${welcome.x} / ${space.x}`);
  // Vertically centered, or filling the workspace and scrolling when it is taller.
  assert.ok(Math.abs(welcome.y - space.y) <= 40, `the start page is centered down: ${welcome.y} / ${space.y}`);
}

async function connectionDialog(page) {
  await editConnection(page, NAME);
  const title = () => page.eval(`${dialog}.querySelector('.dialog-page-title')?.textContent ?? ''`);
  assert.equal(await title(), "General", "the first page is titled");
  const tabs = await page.eval(`[...${dialog}.querySelectorAll('.dialog-tabs button')].map(b => b.textContent.replace('●', '').trim())`);
  assert.ok(tabs.length >= 3, `pages listed: ${tabs}`);
  for (const name of tabs) {
    await page.eval(`[...${dialog}.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.replace('●', '').trim() === ${JSON.stringify(name)}).click()`);
    await page.waitFor(`${dialog}.querySelector('.dialog-page-title')?.textContent === ${JSON.stringify(name)}`);
  }
  // Nothing is set on this connection: no page carries the dot.
  assert.equal(await page.eval(`${dialog}.querySelectorAll('.tab-dot').length`), 0, "no dot while nothing is set");
  // Items of the page list are centered: the text's middle is the button's middle.
  const off = await page.eval(`(() => { const b = ${dialog}.querySelector('.dialog-tabs button'); const r = document.createRange(); r.selectNodeContents(b); const t = r.getBoundingClientRect(); const o = b.getBoundingClientRect(); return Math.abs((t.top + t.bottom) / 2 - (o.top + o.bottom) / 2); })()`);
  assert.ok(off <= 3, `the page names are centered in their buttons: ${off}`);
  // A statement in Init: the dot appears, with its explanation.
  await page.eval(`[...${dialog}.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Init')).click()`);
  await page.waitFor(`${dialog}.querySelector('[data-field=init_statements]') != null`);
  await setField(page, ".connection-dialog [data-field=init_statements]", "SELECT 1");
  await page.waitFor(`${dialog}.querySelectorAll('.tab-dot').length === 1`);
  assert.ok((await page.eval(`${dialog}.querySelector('.tab-dot').getAttribute('aria-label')`)).length > 0, "the dot is explained");
  assert.match(await page.eval(`${dialog}.querySelector('.tab-dot').closest('button').title`), /settings/i);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.connection-dialog') == null`);
}

async function palette(page) {
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') != null && document.querySelectorAll('.palette li').length > 8`);
  const { h } = await page.eval(centerOf(".palette input"));
  assert.ok(h >= 40, `the search input is not squeezed: ${h}px`);
  const normal = await page.eval(`document.querySelector('.sidebar-column input')?.getBoundingClientRect().height ?? 0`);
  assert.ok(h >= normal, `the search input is not shorter than another one: ${h} vs ${normal}`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.palette') == null`);
}

async function emptyProfiles(page) {
  await openProfilesOf(page, NAME);
  const text = await page.eval(`${profilesDialog}.querySelector('.state.empty .state-text')?.textContent ?? ''`);
  assert.match(text, /navigator/i, "the empty state says where a profile is created");
  const label = await page.eval(`${profilesDialog}.querySelector('.state.empty .state-actions button')?.textContent ?? ''`);
  assert.notEqual(label, "", "the empty state has a button");
  await page.eval(`${profilesDialog}.querySelector('.state.empty .state-actions button').click()`);
  await page.waitFor(`${profilesDialog} === undefined`);
  // The button led out of the dialog; opening it again still works and shows the same state.
  await openProfilesOf(page, NAME);
  await closeProfiles(page);
}

const tab = `[...document.querySelectorAll('.dashboard-tab')].find(t => t.style.display !== 'none')`;

async function chartSettings(page) {
  await treeMenu(page, NAME, "New dashboard");
  await page.waitFor(`${tab} != null`);
  await page.eval(`${tab}.querySelector('[data-action="add"]').click()`);
  await page.waitFor(`document.querySelector('.dashboard-form [data-field=script] option[value="regions.sql"]') !== null`, 15_000);
  for (const [field, value] of [["script", "regions.sql"], ["title", "Revenue"], ["view", "chart"]]) {
    await setField(page, `.dashboard-form [data-field=${field}]`, value);
  }
  await page.eval(`document.querySelector('.dashboard-form [data-action=save]').click()`);
  await page.waitFor(`${tab}.querySelector('[data-title="Revenue"] .chart-side') != null`, 30_000);
  const open = async () => {
    await page.eval(`${tab}.querySelector('[data-title="Revenue"] [data-action=menu]').click()`);
    await page.waitFor(`document.querySelector('.context-menu [data-action="dashboard.chart_settings"]') != null`);
  };
  // The settings are open: the item is on, and its mark is an icon (an svg), not a glyph.
  await open();
  const item = `document.querySelector('.context-menu [data-action="dashboard.chart_settings"]')`;
  assert.equal(await page.eval(`${item}.getAttribute('aria-checked')`), "true");
  assert.equal(await page.eval(`${item}.querySelector('svg') != null`), true, "the item shows an svg icon");
  assert.equal(await page.eval(`/[^\\x20-\\x7e\\u00a0-\\uffff]|\\ufffd|\\u0000/.test(${item}.textContent)`), false, "no broken characters");
  await page.eval(`${item}.click()`);
  await page.waitFor(`${tab}.querySelector('[data-title="Revenue"] .chart-side') == null`);
  // Off: no mark.
  await open();
  assert.equal(await page.eval(`${item}.querySelector('svg') == null`), true, "no icon while it is off");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') == null`);
}

export default async function (page) {
  await startPage(page);
  await connectionDialog(page);
  await palette(page);
  await emptyProfiles(page);
  await chartSettings(page);
}
