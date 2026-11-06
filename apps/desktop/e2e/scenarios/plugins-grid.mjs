// Grid extensions (examples/plugins/grid-demo): a computed column, a renderer and a command on a
// cell. The grid draws what they answer, as text on its canvas; markup is an error of its cell and
// reaches no DOM; a row's error stays in that row; a crash and an endless loop show in the column
// and leave the grid working; while masking is on the plugin reads dots, and its answers show it;
// without the plugin, or its permission, the grid is as it always was. A scroll over a large result
// makes batches, not a call per cell, and the figures with and without the extension are logged.
//
// What the crate checks is in crates/platypus-plugins/tests/grid.rs, and the cost of a frame's reads
// in apps/desktop/tests/pluginGrid.test.mjs.
import assert from "node:assert/strict";
import { cpSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { CTRL, ALT } from "../cdp.mjs";
import { openTable, treeRow, visibleTab } from "../query.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const EXAMPLES = resolve(here, "../../../../examples/plugins");
const NAME = "E2E plugin grid";
const ID = "grid-demo";
const DOTS = "••••••";

const panel = `document.querySelector('.plugins-panel')`;
const pluginRow = `${panel}?.querySelector('.driver-block[data-plugin="${ID}"]:not(.plugin-review)')`;
const review = `${panel}?.querySelector('[data-field="plugin-review"]')`;
const toasts = `[...document.querySelectorAll('.toast')].map(t => t.textContent).join(' | ')`;
const menuButton = (label) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)})`;
const scroller = `${visibleTab}?.querySelector('.grid-scroll')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "grid.db"));
  db.exec(`
    CREATE TABLE items (email TEXT, name TEXT, state STATUS);
    INSERT INTO items VALUES ('a@b.c', 'apple', 'ok'), ('boom', 'pear', 'failed'), ('x@y.z', NULL, 'markup'), (NULL, 'kiwi', 'other');
    CREATE TABLE broken (name TEXT);
    INSERT INTO broken VALUES ('crash'), ('fine');
    CREATE TABLE endless (name TEXT);
    INSERT INTO endless VALUES ('endless');
    CREATE TABLE plain (id INTEGER PRIMARY KEY, n INTEGER);
    INSERT INTO plain VALUES (1, 41);
    CREATE TABLE big (name TEXT, state STATUS);
    WITH RECURSIVE c(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM c WHERE i < 200000)
      INSERT INTO big SELECT 'row ' || i, CASE i % 3 WHEN 0 THEN 'ok' WHEN 1 THEN 'failed' ELSE 'other' END FROM c;
  `);
  db.close();
  cpSync(join(EXAMPLES, ID), join(dir, "packages", ID), { recursive: true });
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 8

[[connections]]
id = "00000000-0000-4000-8000-0000000e1001"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "grid.db")}'
save_password = false
`,
  );
  writeFileSync(join(dir, "settings.toml"), `version = 1\n\n[drivers]\nauto_update = false\n`);
}

/** What the canvas draws, and in which colour: it has no text to query. */
async function watchCanvas(page) {
  await page.eval(`(() => {
    window.__drawn = new Map();
    const original = CanvasRenderingContext2D.prototype.fillText;
    CanvasRenderingContext2D.prototype.fillText = function (text, ...rest) {
      window.__drawn.set(String(text), this.fillStyle);
      return original.call(this, text, ...rest);
    };
    // Each command is a request to the IPC's address.
    window.__gridCalls = 0;
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) if (entry.name.includes("plugin_grid_")) window.__gridCalls += 1;
    }).observe({ type: "resource" });
  })()`);
}

const drew = (text) => `window.__drawn.has(${JSON.stringify(text)})`;
const forget = (page) => page.eval(`window.__drawn.clear()`);

async function openPluginsPage(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Plugins");
  await page.waitFor(`${panel} !== null`, 15_000);
}

async function closeSettings(page) {
  await page.eval(`document.querySelector('.modal.settings [role="tab"][aria-selected="true"]')?.focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

async function install(page, dir) {
  await openPluginsPage(page);
  await page.eval(`window.__platypus.answer("folder", ${JSON.stringify(join(dir, "packages", ID))})`);
  await page.eval(`${panel}.querySelector('[data-action="install-folder"]').click()`);
  await page.waitFor(`${review} !== null`, 15_000);
  const text = await page.eval(`${review}.textContent`);
  for (const line of ["Add columns, cell views and cell commands to the result grid", "“Total length” in the result grid", "“Status colours” in the result grid", "“Shout this value” in the result grid"]) {
    assert.ok(text.includes(line), `${line}: ${text}`);
  }
  await page.eval(`${review}.querySelector('[data-action="confirm-install"]').click()`);
  await page.waitFor(`${pluginRow} != null && ${review} === null`, 15_000);
  await closeSettings(page);
}

async function setEnabled(page, on) {
  await openPluginsPage(page);
  const toggle = `${pluginRow}.querySelector('[data-action="toggle"]')`;
  if ((await page.eval(`${toggle}.checked`)) !== on) await page.eval(`${toggle}.click()`);
  await page.waitFor(`${pluginRow}.dataset.enabled === '${on}'`);
  await closeSettings(page);
}

async function closeShownTab(page) {
  const count = await page.eval(`document.querySelectorAll('.pane').length`);
  await page.key("w", "KeyW", 87, CTRL);
  await page.waitFor(`document.querySelectorAll('.pane').length === ${count - 1}`);
}

/** Double-clicks `table` in the navigator, already unfolded, and waits for its `rows`: an exact
 *  status, or just that it says rows, for a result still loading. */
async function openAgain(page, table, rows = null) {
  await page.eval(`${treeRow(table)}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
  await page.waitFor(rows ? `${status} === ${JSON.stringify(rows)}` : `/\\d.*rows/.test(${status})`, 15_000);
}

async function computedAndRendered(page) {
  await watchCanvas(page);
  await openTable(page, NAME, "items", "4 rows");
  // The column, with what it answers for each row.
  await page.waitFor(drew("Total length"), 15_000);
  await page.waitFor(`${drew("10")} && ${drew("5")} && ${drew("4")}`, 15_000);
  // A row's error is that row's.
  await page.waitFor(drew("⚠ this row cannot be counted"));
  // The type of the column it does not read does not count: `state` is not text.
  // The renderer: its colour and its hint, its text in place of the value, others as they are.
  await page.waitFor(`${drew("FAILED")} && ${drew("fine")}`, 15_000);
  assert.equal(await page.eval(`window.__drawn.get('ok')`), "#1a7f37");
  assert.equal(await page.eval(`${drew("failed")}`), true, "drawn before its answer came");
  assert.equal(await page.eval(drew("other")), true);
  // Markup is the error of its cell, and goes nowhere near the DOM.
  await page.waitFor(`[...window.__drawn.keys()].some(k => k.startsWith('⚠ not a cell'))`);
  assert.equal(await page.eval(drew("markup")), true);
  assert.equal(await page.eval(`document.querySelector('.result-grid img, .grid img') === null`), true);
}

/** The context menu of the first column's first row. */
async function cellCommand(page, title) {
  const r = await page.rectOf(scroller);
  await page.click(r.x + 70, r.y + 50, { button: "right" });
  await page.waitFor(`${menuButton("Plugins")} !== undefined`);
  await page.eval(`${menuButton("Plugins")}.click()`);
  await page.waitFor(`${menuButton(title)} !== undefined`);
  await page.eval(`document.querySelectorAll('.toast').forEach(t => t.click())`);
  await page.eval(`${menuButton(title)}.click()`);
  await page.waitFor(`(${toasts}) !== ''`, 15_000);
  return page.eval(toasts);
}

async function maskedForThePlugin(page) {
  assert.match(await cellCommand(page, "Shout this value"), /A@B\.C/);
  // Masking on: `email` shows as dots, and the plugin reads them. It counts 6 for them, and a row
  // it would have refused by its value (`boom`) is a row of dots to it.
  await forget(page);
  await page.key("m", "KeyM", 77, CTRL | ALT);
  await page.waitFor(drew(DOTS), 15_000);
  await page.waitFor(`${drew("11")} && ${drew("10")} && ${drew("6")} && ${drew("4")}`, 15_000);
  assert.equal(await page.eval(drew("a@b.c")), false);
  assert.equal(await page.eval(drew("⚠ this row cannot be counted")), false);
  // A command on the cell reads the dots too.
  assert.ok((await cellCommand(page, "Shout this value")).includes(DOTS));
  await forget(page);
  await page.key("m", "KeyM", 77, CTRL | ALT);
  await page.waitFor(drew("a@b.c"), 15_000);
  await closeShownTab(page);
}

async function failuresStayInTheColumn(page) {
  // A crash: the batch fails, the column says so in its cells, the grid goes on.
  await forget(page);
  await openAgain(page, "broken", "2 rows");
  await page.waitFor(`[...window.__drawn.keys()].some(k => k.startsWith('⚠') && k.includes('stopped with an error'))`, 15_000);
  await page.waitFor(`${drew("crash")} && ${drew("fine")}`);
  await page.waitFor(`(${toasts}).includes('“Total length” of the plugin “Grid demo” failed')`, 15_000);
  await closeShownTab(page);

  // An endless loop: the fuel stops it, within the time of a batch, and the app never waits.
  await forget(page);
  const start = Date.now();
  await openAgain(page, "endless", "1 row");
  await page.waitFor(`[...window.__drawn.keys()].some(k => k.startsWith('⚠'))`, 20_000);
  console.log(`an endless computed column showed its error after ${Date.now() - start} ms`);
  assert.equal(await page.eval(drew("endless")), true);
  await closeShownTab(page);
}

async function typesTheyDoNotReadStayAsTheyAre(page) {
  await forget(page);
  // The observer hears of a request a moment after it ends.
  await page.sleep(500);
  const before = await page.eval(`window.__gridCalls`);
  await openAgain(page, "plain", "1 row");
  await page.waitFor(drew("41"));
  await page.sleep(700);
  assert.equal(await page.eval(drew("Total length")), false, "a column for text on a table without any");
  assert.equal(await page.eval(`window.__gridCalls`), before);
  await closeShownTab(page);
}

/** Scrolls the big table by frames: what a frame takes, and how many calls it made. */
async function scrollCost(page) {
  return page.eval(`new Promise((done) => {
    const el = ${scroller};
    const calls = window.__gridCalls;
    const times = [];
    let last = performance.now();
    let frame = 0;
    const step = (now) => {
      times.push(now - last);
      last = now;
      el.scrollTop += 4000;
      frame += 1;
      if (frame < 240) return requestAnimationFrame(step);
      const sorted = times.slice(1).sort((a, b) => a - b);
      const result = { frames: frame, mean: sorted.reduce((a, b) => a + b, 0) / sorted.length, p95: sorted[Math.floor(sorted.length * 0.95)] };
      // The observer hears of a request a moment after it ends.
      setTimeout(() => done({ ...result, calls: window.__gridCalls - calls }), 500);
    };
    requestAnimationFrame(step);
  })`);
}

async function scrollingALargeResult(page) {
  await forget(page);
  await openAgain(page, "big");
  await page.waitFor(`${drew("Total length")}`, 15_000);
  const withExtension = await scrollCost(page);
  await page.sleep(300);
  const again = await scrollCost(page);
  console.log(`scroll over 200000 rows with extensions: ${JSON.stringify(withExtension)}, again: ${JSON.stringify(again)}`);
  // Batches, one at a time per extension: never a call for each cell of each frame, which would
  // be some 90 per frame here.
  assert.ok(withExtension.calls > 0 && withExtension.calls <= withExtension.frames * 2, JSON.stringify(withExtension));
  await closeShownTab(page);

  await setEnabled(page, false);
  await openAgain(page, "big");
  await page.sleep(300);
  const without = await scrollCost(page);
  console.log(`scroll over 200000 rows without: ${JSON.stringify(without)}`);
  assert.equal(without.calls, 0);
  await closeShownTab(page);
  await setEnabled(page, true);
}

async function goneWithoutThePluginOrItsPermission(page, dir) {
  // Off: nothing is added, and the table is what it always was.
  await setEnabled(page, false);
  await forget(page);
  await page.sleep(500);
  const before = await page.eval(`window.__gridCalls`);
  await openAgain(page, "items", "4 rows");
  await page.waitFor(drew("apple"));
  await page.sleep(700);
  assert.equal(await page.eval(drew("Total length")), false);
  assert.equal(await page.eval(drew("FAILED")), false);
  assert.equal(await page.eval(`window.__gridCalls`), before);
  await closeShownTab(page);

  // On, with the permission taken back in the file the backend reads.
  const file = join(dir, "plugins", "plugins.json");
  const state = JSON.parse(readFileSync(file, "utf8"));
  state.plugins[ID].approved.grid = false;
  writeFileSync(file, JSON.stringify(state));
  await setEnabled(page, true);
  await forget(page);
  await openAgain(page, "items", "4 rows");
  await page.waitFor(drew("apple"));
  await page.sleep(700);
  assert.equal(await page.eval(drew("Total length")), false);
  assert.equal(await page.eval(`window.__gridCalls`), before);
  // Nothing in the menu either.
  const r = await page.rectOf(scroller);
  await page.click(r.x + 70, r.y + 50, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') != null`);
  assert.equal(await page.eval(`${menuButton("Plugins")} === undefined || (${menuButton("Plugins")}.click(), ${menuButton("Shout this value")} === undefined)`), true);
}

export default async function (page, { dir }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await install(page, dir);
  await computedAndRendered(page);
  await maskedForThePlugin(page);
  await failuresStayInTheColumn(page);
  await typesTheyDoNotReadStayAsTheyAre(page);
  await scrollingALargeResult(page);
  await goneWithoutThePluginOrItsPermission(page, dir);
}
