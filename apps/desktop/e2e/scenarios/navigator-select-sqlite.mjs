// Navigator rows on a throwaway SQLite database: a click selects (Ctrl and Shift for
// several), only the arrow expands, a double-click opens, and actions run on the whole
// selection. The data viewer reads a large table a page at a time.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, SHIFT } from "../cdp.mjs";

const NAME = "Select db";
const ROWS = 5000;
/** Columns enough to reach under the vertical scrollbar. */
const WIDE = Array.from({ length: 12 }, (_, i) => `w${i + 1}`);
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const selected = `[...document.querySelectorAll('.tree-row.selected')].map(r => r.querySelector('.tree-label').textContent)`;
const queryTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${queryTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const loaded = `Number(${status}.split(' ')[0].replace(/\\D/g, ''))`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "select.db"));
  db.exec(`CREATE TABLE a (id INTEGER PRIMARY KEY);
           CREATE TABLE b (id INTEGER PRIMARY KEY);
           CREATE TABLE c (id INTEGER PRIMARY KEY, v TEXT, ${WIDE.map((w) => `${w} TEXT`).join(", ")});
           WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < ${ROWS})
           INSERT INTO c SELECT n, 'value ' || n, ${WIDE.map(() => "printf('%040d', n)").join(", ")} FROM s;`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000011"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "select.db")}'
save_password = false
`,
  );
  writeFileSync(join(dir, "settings.toml"), '[navigator]\nview = "simple"\n');
}

/** Clicks the row's label, with `options` (modifiers, button, clickCount). */
async function clickRow(page, label, options = {}) {
  await page.eval(`${row(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", options);
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
}

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  // Nothing connects on its own at startup: no tab is open.
  await page.sleep(1500);
  const log = await page.eval(`window.__TAURI_INTERNALS__.invoke('query_log', { since: 0 }).then(p => (p.entries ?? p).length)`);
  assert.equal(log, 0, "no statement before a click");
  assert.equal(await page.eval(`document.querySelector('.connected-mark')`), null);
  // A click on the label selects the connection but does not expand it; the arrow does.
  await clickRow(page, NAME);
  assert.deepEqual(await page.eval(selected), [NAME]);
  assert.equal(await page.eval(`${row(NAME)}.getAttribute('aria-expanded')`), "false");
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("a")} !== undefined && ${row("c")} !== undefined`, 15_000);
  await page.waitFor(`${row(NAME)}.querySelector('.connected-mark') !== null`);

  // Shift+click selects the range from the last click, Ctrl+click adds or removes a row.
  await clickRow(page, "a");
  await clickRow(page, "c", { modifiers: SHIFT });
  assert.deepEqual(await page.eval(selected), ["a", "b", "c"]);
  await clickRow(page, "b", { modifiers: CTRL });
  assert.deepEqual(await page.eval(selected), ["a", "c"]);

  // The menu of a selection lists only what applies to all of it; DDL covers both tables.
  await clickRow(page, "c", { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  const items = await page.eval(`[...document.querySelectorAll('.context-menu button')].map(b => b.textContent)`);
  assert.ok(items.some((i) => i.startsWith("Generate DDL")), `DDL in ${items}`);
  assert.ok(!items.some((i) => i.startsWith("Drop")), `no Drop in ${items}`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Generate DDL')).click()`);
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('DDL of 2 objects')`, 10_000);
  // The tab opens at once and fills as the DDL is read.
  await page.waitFor(`${queryTab}?.querySelector('.ddl-progress') == null && ${queryTab}.querySelectorAll('.editor .cm-line').length > 1`, 10_000);
  const ddl = await page.eval(`[...${queryTab}.querySelectorAll('.editor .cm-line')].map(l => l.textContent).join('\\n')`);
  assert.match(ddl, /CREATE TABLE .*\ba\b/i);
  assert.match(ddl, /CREATE TABLE .*\bc\b/i);

  // A double-click opens the data and leaves the row as it was.
  await clickRow(page, "c", { clickCount: 2 });
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('c')`, 10_000);
  assert.equal(await page.eval(`${row("c")}.getAttribute('aria-expanded')`), "false");
  // Again from another tab: back to the same one, no second tab.
  const tabs = await page.eval(`document.querySelectorAll('.tab').length`);
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.startsWith('DDL')).click()`);
  await clickRow(page, "c", { clickCount: 2 });
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('c')`, 10_000);
  assert.equal(await page.eval(`document.querySelectorAll('.tab').length`), tabs, "the open tab is reused");

  // Only a page of the table is read; scrolling to the end reads more.
  // The status says "0 rows loaded · more available" from the start: wait for rows.
  await page.waitFor(`${status}.includes('more available') && ${loaded} > 0`, 15_000);
  const first = await page.eval(loaded);

  // Dragging the scrollbar thumb from over the header row does not sort that column.
  await page.eval(`${queryTab}.querySelector('.grid-scroll').id = 'e2e-grid'`);
  const scrollbar = `(() => { const s = document.getElementById('e2e-grid'); return s.offsetWidth - s.clientWidth; })()`;
  // The grid draws the rows a moment after the status counts them.
  await page.waitFor(`${scrollbar} > 0`, 5_000).catch(() => {});
  const g = await page.rect("#e2e-grid");
  const bar = await page.eval(scrollbar);
  assert.ok(bar > 0, "the grid has a vertical scrollbar");
  const x = g.x + g.w - bar / 2;
  await page.mouse("mousePressed", x, g.y + 8);
  await page.mouse("mouseMoved", x, g.y + 40);
  await page.mouse("mouseReleased", x, g.y + 40);
  await page.sleep(500);
  const sql = await page.eval(`[...${queryTab}.querySelectorAll('.editor .cm-line')].map(l => l.textContent).join(' ')`);
  assert.doesNotMatch(sql, /ORDER BY/, "no sort from the scrollbar");
  assert.ok(first > 0 && first < ROWS, `a page of rows, not ${first}`);
  await page.waitFor(
    `(() => { const g = ${queryTab}.querySelector('.grid-scroll'); g.scrollTop = g.scrollHeight; return ${loaded} > ${first}; })()`,
    15_000,
  );
}
