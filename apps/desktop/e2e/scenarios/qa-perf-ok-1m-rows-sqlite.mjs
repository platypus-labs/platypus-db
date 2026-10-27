// Guard: a 1,000,000-row result. The first page arrives at once, "Fetch all" loads every row
// within seconds without freezing the window, Ctrl+End reaches the last row, and "Copy with
// headers" of the whole selection (built by the backend) holds every row.
// Measured on the dev machine: fetch all 1.2 s (SQLite), 1.4 s (MySQL), 2.4 s (PostgreSQL),
// 5.3 s (SQL Server), 0.4 s (DuckDB); 5M PostgreSQL rows 20 s at ~570 MB.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, SHIFT } from "../cdp.mjs";
import { clipboardAfter, grid, gridAt, lag, openEditor, run, saveConnections, sqliteEntry, status, visibleTab } from "../perf.mjs";

const NAME = "QA perf 1M";
const ROWS = 1_000_000;

export function setup({ dir }) {
  const file = join(dir, "million.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE big (id INTEGER PRIMARY KEY, name TEXT, amount REAL, note TEXT);
    WITH RECURSIVE s(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM s WHERE g < ${ROWS})
    INSERT INTO big SELECT g, 'name ' || g, (g % 100000) / 100.0, printf('%.40c', 'x') || g FROM s;`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

export default async function (page) {
  await openEditor(page, NAME);
  let started = performance.now();
  await run(page, "SELECT id, name, amount, note FROM big ORDER BY id");
  console.log(`  first page: ${Math.round(performance.now() - started)} ms`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  started = performance.now();
  await page.eval(`[...${visibleTab}.querySelectorAll('.statusbar button')].find(b => b.textContent === 'Fetch all').click()`);
  let worst = 0;
  while (!(await page.eval(`/^[\\d.,]+ rows$/.test(${status}.trim())`))) {
    worst = Math.max(worst, await lag(page));
    assert.ok(performance.now() - started < 30_000, "fetch all within 30 s");
    await page.sleep(100);
  }
  const fetched = performance.now() - started;
  console.log(`  fetch all: ${Math.round(fetched)} ms, worst lag ${Math.round(worst)} ms`);
  assert.match(await page.eval(status), /^1[.,]?000[.,]?000 rows$/);
  assert.ok(worst < 500, `the window froze ${Math.round(worst)} ms while fetching`);

  const g = await gridAt(page);
  await page.click(g.x + 200, g.y + 60);
  await page.key("End", "End", 35, CTRL);
  await page.waitFor(`${grid}.scrollTop > 0`);
  // The last block arrives once scrolled to; the plain copy reads what the grid holds.
  let last = "";
  for (let i = 0; i < 50 && last === ""; i++) {
    last = await clipboardAfter(page, "c", "KeyC", 67, CTRL);
    if (last === "") await page.sleep(100);
  }
  assert.equal(last, `${"x".repeat(40)}${ROWS}`);

  await page.key("a", "KeyA", 65, CTRL);
  started = performance.now();
  const all = await clipboardAfter(page, "C", "KeyC", 67, CTRL | SHIFT, 60_000);
  const lines = all.replace(/\r?\n$/, "").split(/\r?\n/);
  console.log(`  copy with headers of ${ROWS} rows: ${Math.round(performance.now() - started)} ms, ${Math.round(all.length / 1e6)} MB`);
  assert.equal(lines.length, ROWS + 1);
  assert.equal(lines[0], "id\tname\tamount\tnote");
  assert.equal(lines[ROWS / 2 + 1], `${ROWS / 2 + 1}\tname ${ROWS / 2 + 1}\t0.01\t${"x".repeat(40)}${ROWS / 2 + 1}`);
}
