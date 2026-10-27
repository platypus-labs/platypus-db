// BUG (low, misleading timing): after "Fetch all" the status says "took N s · slow" where N counts
// from the start of the statement, including the time the result sat paused at its first page
// waiting for the user. Opening a 1M-row PostgreSQL table, waiting a minute and clicking Fetch
// all (2.4 s of fetching) showed "took 87.85 s · slow". The time should be the time spent
// running/fetching (or the first-page time and the fetch time separately).
//   Steps: SELECT a 5,000-row table (first page 1,500 rows), wait 5 s, Fetch all.
//   Expected: "took" well under the 5 s of waiting (the fetch takes milliseconds).
//   Actual: "took 5.xx s" and "slow".
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fetchAll, openEditor, run, saveConnections, sqliteEntry, statusbar } from "../perf.mjs";

const NAME = "QA perf took";

export function setup({ dir }) {
  const file = join(dir, "took.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
    WITH RECURSIVE s(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM s WHERE g < 5000)
    INSERT INTO items SELECT g, 'item ' || g FROM s;`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

export default async function (page) {
  await openEditor(page, NAME);
  await run(page, "SELECT id, name FROM items");
  assert.match(await page.eval(statusbar), /more available/);
  await page.sleep(5000);
  const started = performance.now();
  await fetchAll(page);
  const fetched = performance.now() - started;
  const text = await page.eval(statusbar);
  const took = text.match(/took ([\d.,]+) (ms|s)/);
  assert.ok(took, text);
  const ms = Number(took[1].replace(",", ".")) * (took[2] === "s" ? 1000 : 1);
  console.log(`  fetch all took ${Math.round(fetched)} ms; the status says "${took[0]}"`);
  assert.ok(ms < 2000, `the status says "${took[0]}" for a fetch of ${Math.round(fetched)} ms (${text})`);
  assert.doesNotMatch(text, /slow/);
}
