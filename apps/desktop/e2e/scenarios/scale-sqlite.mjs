// Scale: a SQLite database of 10,000 tables in the navigator and the editor. The navigator
// renders only the rows around its view, so expanding the connection, selecting, filtering
// and moving with the keyboard stay quick; the rows not rendered keep their place in the
// scrollbar, the keyboard reaches them and a Shift+click selects across them. Completion
// lists tables and columns among the 10,000 at once.
// Measured in Chromium with the app's frontend on a fake backend: expanding 3.7 s and a
// click 0.5 s before, 0.2 s and 0.07 s after (ROADMAP, group P).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, SHIFT } from "../cdp.mjs";
import { saveConnections, sqliteEntry } from "../perf.mjs";

const NAME = "Scale 10k";
const TABLES = 10_000;
const table = (i) => `scale_t_${String(i).padStart(5, "0")}`;
const LAST = table(TABLES);
/** What a user notices; the dev machine is several times faster. */
const EXPAND_MS = 3000;
const CLICK_MS = 500;
const COMPLETE_MS = 2000;

const rows = `[...document.querySelectorAll('.tree-row')]`;
const row = (label) => `${rows}.find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const tree = `document.querySelector('.tree')`;
const focused = `document.activeElement?.closest('.tree-row')?.querySelector('.tree-label')?.textContent`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const options = `[...document.querySelectorAll('.cm-tooltip-autocomplete li')].map(li => li.textContent)`;

export function setup({ dir }) {
  const file = join(dir, "scale.db");
  const db = new DatabaseSync(file);
  db.exec("BEGIN");
  for (let i = 1; i <= TABLES; i++) {
    db.exec(`CREATE TABLE ${table(i)} (id INTEGER PRIMARY KEY, name TEXT, amount REAL)`);
  }
  db.exec("COMMIT");
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

/** Milliseconds from running `act` in the page to the first frame where `done` holds. */
async function timed(page, act, done, timeout = 60_000) {
  const started = performance.now();
  await page.eval(act);
  await page.waitFor(done, timeout);
  return Math.round(performance.now() - started);
}

/** Clicks the label of the navigator row `label`, scrolled into view, with `modifiers`. */
async function clickRow(page, label, modifiers = 0) {
  await page.eval(`document.querySelectorAll('#e2e-row').forEach(e => e.removeAttribute('id'));
    ${row(label)}.querySelector('.tree-label').id = 'e2e-row'; document.getElementById('e2e-row').scrollIntoView({ block: 'nearest' })`);
  await page.clickSel("#e2e-row", { modifiers });
}

/** Replaces the editor's text with `sql` and types `typed` after it key by key. */
async function typeAfter(page, sql, typed) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27);
  for (const ch of typed) {
    await page.type(ch);
    await page.sleep(20);
  }
  await page.waitFor(`${editorText} === ${JSON.stringify(sql + typed)}`);
}

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);

  // Expanding the connection opens its tables at once: 10,000 of them, a few hundred rendered.
  const expand = await timed(
    page,
    `${row(NAME)}.querySelector('.twisty').click()`,
    `${rows}.some(r => r.querySelector('.tree-label')?.textContent === 'Tables' && r.querySelector('.tree-detail')?.textContent === '${TABLES}') && ${row(table(1))} !== undefined`,
  );
  console.log(`  expanding ${TABLES} tables: ${expand} ms`);
  assert.ok(expand < EXPAND_MS, `expanding took ${expand} ms`);
  const rendered = await page.eval(`${rows}.length`);
  assert.ok(rendered > 100 && rendered < 1000, `${rendered} rows rendered`);
  // The scrollbar spans every table: 28 px a row.
  assert.ok(await page.eval(`${tree}.scrollHeight >= ${TABLES} * 28`), "the scroll height counts every row");
  assert.equal(await page.eval(`${row(LAST)} === undefined`), true, "the last table is not rendered yet");

  // A click selects at once, however long the tree.
  await clickRow(page, table(5));
  await page.waitFor(`${row(table(5))}.classList.contains('selected')`);
  const click = await timed(page, `${row(table(7))}.dispatchEvent(new MouseEvent('click', { bubbles: true }))`, `${row(table(7))}.classList.contains('selected') && !${row(table(5))}.classList.contains('selected')`);
  console.log(`  selecting a table: ${click} ms`);
  assert.ok(click < CLICK_MS, `a click took ${click} ms`);

  // Shift+click selects the range, also the rows between that were never rendered.
  await clickRow(page, table(2));
  await page.waitFor(`${row(table(2))}.classList.contains('selected')`);
  await page.eval(`${tree}.scrollTop = 5000 * 28`);
  await page.waitFor(`${row(table(5000))} !== undefined`);
  await clickRow(page, table(5000), SHIFT);
  await page.waitFor(`${row(table(5000))}.classList.contains('selected')`);
  await page.eval(`${tree}.scrollTop = 0`);
  await page.waitFor(`${row(table(2))} !== undefined`);
  assert.equal(await page.eval(`${row(table(150))}.classList.contains('selected')`), true, "the range holds the rows between");
  assert.equal(await page.eval(`${row(table(1))}.classList.contains('selected')`), false, "the row before the range is not selected");

  // A focused row scrolled out of the rendered ones leaves the focus in the tree: the keys
  // still move from it.
  await clickRow(page, table(1));
  await page.waitFor(`${row(table(1))}.classList.contains('selected') && ${focused} === ${JSON.stringify(table(1))}`);
  await page.eval(`${tree}.scrollTop = 7000 * 28`);
  await page.waitFor(`${row(table(1))} === undefined && document.activeElement === ${tree}`);
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.waitFor(`${focused} === ${JSON.stringify(table(2))}`, 5000);

  // End reaches the last table, rendered and focused; Up moves from there; Home goes back.
  await page.key("End", "End", 35);
  await page.waitFor(`${focused} === ${JSON.stringify(LAST)}`, 5000);
  assert.ok(await page.eval(`${row(LAST)}.classList.contains('selected')`), "End selects the last table");
  assert.ok(await page.eval(`${rows}.length < 1000`), "the rows rendered stay bounded at the end");
  await page.key("ArrowUp", "ArrowUp", 38);
  await page.waitFor(`${focused} === ${JSON.stringify(table(TABLES - 1))}`);
  await page.key("Home", "Home", 36);
  await page.waitFor(`${focused} === ${JSON.stringify(NAME)}`, 5000);
  assert.equal(await page.eval(`${tree}.scrollTop`), 0, "Home scrolls back to the top");

  // Scrolling to the bottom with the scrollbar renders the last rows.
  await page.eval(`${tree}.scrollTop = ${tree}.scrollHeight`);
  await page.waitFor(`${row(LAST)} !== undefined`);
  await page.eval(`${tree}.scrollTop = 0`);
  await page.waitFor(`${row(table(1))} !== undefined`);

  // The filter: one table, then none, then all of them again with Escape.
  const filter = `document.querySelector('.tree-filter')`;
  await page.eval(`${filter}.focus()`);
  await page.type(table(9999).slice(2));
  await page.waitFor(`${row(table(9999))} !== undefined && ${rows}.length < 10`, 10_000);
  assert.equal(await page.eval(`${row(table(9998))} === undefined`), true);
  await page.type("x");
  await page.waitFor(`${row(table(9999))} === undefined`, 10_000);
  assert.equal(await page.eval(`${row(NAME)} !== undefined`), true, "the connection stays when nothing matches");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${filter}.value === '' && ${row(table(1))} !== undefined`, 10_000);

  // Completion among 10,000 tables: the tables by prefix, then a table's columns.
  await page.eval(`${row(NAME)}.id = 'e2e-connection'; document.getElementById('e2e-connection').scrollIntoView({ block: 'nearest' })`);
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  let started = performance.now();
  await typeAfter(page, "SELECT * FROM ", "scale_t_0999");
  await page.waitFor(`${options}.some(o => o.startsWith('scale_t_09990'))`, COMPLETE_MS * 5);
  let took = Math.round(performance.now() - started);
  console.log(`  table completion: ${took} ms`);
  const tables = await page.eval(options);
  assert.ok(tables.some((o) => o.startsWith(table(9999))), `tables: ${tables.slice(0, 12)}`);
  assert.ok(!tables.some((o) => o.startsWith(table(1))), "the list keeps to the prefix");
  // CodeMirror ignores keys for 75 ms after the list opens.
  await page.sleep(200);
  await page.key("Escape", "Escape", 27);

  started = performance.now();
  await typeAfter(page, `SELECT ${LAST}.`, "am");
  await page.waitFor(`${options}.some(o => o.startsWith('amount'))`, COMPLETE_MS * 5);
  took = Math.round(performance.now() - started);
  console.log(`  column completion: ${took} ms`);
  assert.ok(took < COMPLETE_MS * 2, `column completion took ${took} ms`);

  // A table that is not there completes nothing, without an error.
  await page.key("Escape", "Escape", 27);
  await typeAfter(page, "SELECT scale_t_99999.", "a");
  await page.sleep(600);
  assert.deepEqual(await page.eval(options), [], "no columns for a missing table");
}
