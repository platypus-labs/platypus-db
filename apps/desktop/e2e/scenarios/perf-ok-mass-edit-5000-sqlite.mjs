// Guard: 5,000 cell edits at once (Edit selection › Set value… on a column of 5,000 rows) apply
// and save quickly, and every row changes in the database. Measured: apply 0.2 s, save 0.5 s
// (PostgreSQL), the window responsive throughout.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { SHIFT } from "../cdp.mjs";
import { fetchAll, grid, gridAt, lag, openEditor, run, saveConnections, sqliteEntry, visibleTab } from "../perf.mjs";

const NAME = "QA perf mass edit";
const ROWS = 5000;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;

export function setup({ dir }) {
  const file = join(dir, "edit.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, city TEXT);
    WITH RECURSIVE s(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM s WHERE g < ${ROWS})
    INSERT INTO people SELECT g, 'name ' || g, 'city ' || (g % 10) FROM s;`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

export default async function (page, { dir }) {
  await openEditor(page, NAME);
  await run(page, "SELECT id, name, city FROM people ORDER BY id");
  await fetchAll(page);
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  // The name column, from the first row to the last.
  const g = await gridAt(page);
  const x = await page.eval(`(() => { const r = ${grid}.getBoundingClientRect(); return r.x; })()`);
  await page.click(x + 130, g.y + 60);
  const pageRows = await page.eval(`Math.floor((${grid}.clientHeight - 48) / 24) - 1`);
  for (let i = 0; i < Math.ceil(ROWS / pageRows) + 2; i++) await page.key("PageDown", "PageDown", 34, SHIFT);
  await page.click(x + 130, g.y + g.h - 40, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Edit selection')).click()`);
  await page.waitFor(`[...document.querySelectorAll('.context-menu button')].some(b => b.textContent.startsWith('Set value'))`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Set value')).click()`);
  const dialog = `document.querySelector('.modal.mass-edit')`;
  await page.waitFor(`${dialog} !== null`);
  await page.eval(`${dialog}.querySelector('[aria-label="Value"]').focus()`);
  await page.type("edited");
  await page.waitFor(`/5[.,]?000 cells will change/.test(${dialog}.textContent)`, 15_000);
  let started = performance.now();
  await page.eval(`${dialog}.querySelector('button.primary').click()`);
  await page.waitFor(`/5[.,]?000 rows changed/.test(${editBar})`, 30_000);
  const applied = performance.now() - started;
  started = performance.now();
  await page.eval(`${visibleTab}.querySelector('.edit-bar button.primary').click()`);
  let worst = 0;
  while (!(await page.eval(`${editBar}.includes('to edit a cell')`))) {
    worst = Math.max(worst, await lag(page));
    assert.ok(performance.now() - started < 30_000, "saved within 30 s");
    await page.sleep(50);
  }
  const saved = performance.now() - started;
  console.log(`  ${ROWS} edits: apply ${Math.round(applied)} ms, save ${Math.round(saved)} ms, worst lag ${Math.round(worst)} ms`);
  assert.ok(applied < 3000 && saved < 5000, `apply ${Math.round(applied)} ms, save ${Math.round(saved)} ms`);
  const db = new DatabaseSync(join(dir, "edit.db"));
  const row = { ...db.prepare("SELECT count(*) AS n FROM people WHERE name = 'edited'").get() };
  db.close();
  assert.equal(row.n, ROWS);
}
