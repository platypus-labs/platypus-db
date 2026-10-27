// Deleting many rows in the data editor, against the dev PostgreSQL from docker/compose.yml:
// marking a thousand rows and saving them stays quick, and the preview shows the batched
// statements that run. Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { ALT, CTRL } from "../cdp.mjs";

export { setup, skip } from "./dev-postgres.mjs";

const NAME = "E2E PG";
const TABLE = "demo.e2e_many";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;
const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
/** Generous bounds: they catch a return to one round trip per row, not a slow runner. */
const MARK_LIMIT_MS = 2_000;
const SAVE_LIMIT_MS = 10_000;

/** Replaces the editor's text and runs it (`script`: all of it, else the statement). */
async function run(page, sql, script = false) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes(${JSON.stringify(sql.slice(-20))})`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  if (script) await page.key("x", "KeyX", 88, ALT);
  else await page.key("Enter", "Enter", 13, CTRL);
  const idle = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')?.disabled`;
  await page.waitFor(`${idle} && ${status} !== '' && !${status}.includes('Executing')`, 30_000);
}

/** Creates the table afresh with `n` rows, from the SQL editor's tab. */
async function create(page, n) {
  await run(
    page,
    `DROP TABLE IF EXISTS ${TABLE};\nCREATE TABLE ${TABLE} (id uuid PRIMARY KEY DEFAULT gen_random_uuid(), ` +
      "code text, label_ita text, label_eng text);\n" +
      `INSERT INTO ${TABLE} (code, label_ita, label_eng) ` +
      `SELECT 'c' || g, 'ita ' || g, 'eng ' || g FROM generate_series(1, ${n}) g;`,
    true,
  );
}

/** Marks the visible grid's `n` rows for deletion; returns how long marking took. */
async function markAll(page, n) {
  await page.waitFor(`${status}.replace(/\\D/g, '') === '${n}'`, 30_000);
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await page.eval(`${grid}.focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  const started = performance.now();
  await page.key("Delete", "Delete", 46);
  await page.waitFor(`${editBar}.replace(/[^\\d a-z]/g, '').includes('${n} rows changed')`, 30_000);
  return performance.now() - started;
}

/** Creates `n` rows, selects them in the SQL editor and marks them all for deletion. */
async function markQueried(page, n) {
  await create(page, n);
  await run(page, `SELECT * FROM ${TABLE}`);
  return markAll(page, n);
}

/** Saves the pending changes and waits until the grid has none; returns how long it took. */
async function save(page) {
  const started = performance.now();
  await page.eval(`${visibleTab}.querySelector('.edit-bar button.primary').click()`);
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 60_000);
  return performance.now() - started;
}

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  await page.waitFor(`${row("demo")} !== undefined`, 15_000);
  await page.eval(`${row(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);

  for (const n of [100, 1000]) {
    const mark = await markQueried(page, n);
    const saved = await save(page);
    console.log(`  ${n} rows: marking ${Math.round(mark)} ms, saving ${Math.round(saved)} ms`);
    assert.ok(mark < MARK_LIMIT_MS, `marking ${n} rows took ${Math.round(mark)} ms`);
    assert.ok(saved < SAVE_LIMIT_MS, `saving ${n} deleted rows took ${Math.round(saved)} ms`);
    await run(page, `SELECT count(*) AS left_rows FROM ${TABLE}`);
    // The count's grid is in place once its bar says it cannot be edited (the table's was).
    await page.waitFor(`${editBar}.includes('Read-only')`, 15_000);
    await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
    await page.eval(`window.__copied = undefined; ${grid}.focus()`);
    await page.key("a", "KeyA", 65, CTRL);
    await page.key("c", "KeyC", 67, CTRL);
    await page.waitFor(`window.__copied !== undefined`);
    assert.equal(await page.eval(`window.__copied`), "0", `${n} rows deleted`);
  }

  // The same in the data viewer of the table, opened from the navigator.
  await create(page, 1000);
  await page.eval(`${row("demo")}.querySelector(".twisty").click()`);
  await page.waitFor(`${row("e2e_many")} !== undefined`, 15_000);
  await page.eval(`${row("e2e_many")}.id = 'e2e-many'`);
  const r = await page.rect("#e2e-many");
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent === 'Open data').click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null`);
  await page.waitFor(`${visibleTab}.querySelector('.result-loading') === null && ${status}.includes('rows')`, 30_000);
  const viewerMark = await markAll(page, 1000);
  const viewerSave = await save(page);
  console.log(`  1000 rows in the data viewer: marking ${Math.round(viewerMark)} ms, saving ${Math.round(viewerSave)} ms`);
  assert.ok(viewerMark < MARK_LIMIT_MS, `marking took ${Math.round(viewerMark)} ms`);
  assert.ok(viewerSave < SAVE_LIMIT_MS, `saving took ${Math.round(viewerSave)} ms`);
  await page.waitFor(`${status}.replace(/\\D/g, '') === '0'`, 15_000);
  // Back to the SQL editor's tab.
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.startsWith('E2E PG'))?.click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null && ${visibleTab}.querySelector('.viewer-bar') == null`);

  // The preview shows the statements that run: the keys in lists of 500.
  await markQueried(page, 1000);
  await page.eval(`${visibleTab}.querySelector('.edit-bar button[aria-label="Preview SQL"]').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') !== null`);
  const sql = await page.eval(`document.querySelector('.sql-preview').textContent`);
  const statements = sql.split(";\n");
  assert.equal(statements.length, 2, sql.slice(0, 300));
  for (const statement of statements) {
    assert.match(statement, /^DELETE FROM demo\.e2e_many WHERE id IN \('[0-9a-f-]{36}'(, '[0-9a-f-]{36}'){499}\);?$/);
  }
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
  await page.eval(`${visibleTab}.querySelector('.edit-bar button[aria-label="Discard"]').click()`);
  await page.waitFor(`${editBar}.includes('to edit a cell')`);
  await run(page, `DROP TABLE ${TABLE}`);
}
