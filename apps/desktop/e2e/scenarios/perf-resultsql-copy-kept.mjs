// BUG (medium, memory): the in-memory DuckDB copy that "SQL on the loaded rows" makes of a result
// stays after the tab runs another statement, until the tab is closed or × is pressed on a
// field nobody sees any more. Fields.forget() (src/analysis/ResultSql.tsx) drops the output
// but keeps the copy for a later reload. Measured: 1M rows (id, md5, 100-char pad) hold
// ~170 MB after the tab ran `SELECT 1` twice (32 MB without the field); 5M rows ~450 MB.
//   Steps: a 1,000,000-row result, fetch all, run `WHERE id % 2 = 0` in the field above the
//   grid, then run `SELECT 1` in the editor twice.
//   Expected: the app's memory falls back near where it was before the big result.
//   Actual: it stays ~150 MB higher until the tab is closed.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fetchAll, memory, openEditor, run, saveConnections, sqliteEntry, visibleTab } from "../perf.mjs";

const NAME = "QA perf result copy";

export function setup({ dir }) {
  const file = join(dir, "big.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE big (id INTEGER PRIMARY KEY, hash TEXT, pad TEXT);
    WITH RECURSIVE s(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM s WHERE g < 1000000)
    INSERT INTO big SELECT g, hex(randomblob(16)), printf('%.100c', 'y') FROM s;`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

const settle = (page) => page.sleep(2000);

export default async function (page, { dir }) {
  await openEditor(page, NAME);
  await run(page, "SELECT 1");
  await settle(page);
  const base = memory(dir).app;
  await run(page, "SELECT id, hash, pad FROM big");
  await fetchAll(page);
  const field = `${visibleTab}.querySelector('.result-sql .cm-content')`;
  await page.eval(`${field}.focus()`);
  await page.type("WHERE id % 2 = 0");
  await page.waitFor(`${field}.textContent === 'WHERE id % 2 = 0'`);
  await page.key("Escape", "Escape", 27);
  await page.eval(`${field}.focus()`);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`/SQL view/.test(${visibleTab}.querySelector('.result-sql').textContent)`, 60_000);
  await settle(page);
  const withCopy = memory(dir).app;
  // The tab runs other statements: the result and the field's output go.
  await run(page, "SELECT 1");
  await run(page, "SELECT 2");
  await settle(page);
  const after = memory(dir).app;
  console.log(`  app memory: ${base} MB before, ${withCopy} MB with the copy, ${after} MB after two more runs`);
  assert.ok(after - base < 60, `the app still holds ${after - base} MB more than before the result`);
}
