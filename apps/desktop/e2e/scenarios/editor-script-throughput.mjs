// SQL-14. Long scripts slow down as they go: every statement adds a result tab and the whole
// list is updated per statement, so the rate falls with the number of statements already run.
// Measured on PostgreSQL in Docker: 1000 INSERTs took 40 s, from ~36 to ~14 statements/s;
// 2000 INSERTs on a local SQLite file took 147 s, against 4.6 s for the same INSERTs run by
// node:sqlite. A dump of 10k statements is not practical.
// Steps: SQLite, run as a script 900 `INSERT INTO t ...;` into a TEMP table (no disk work).
// Measured here: 34.8 s in all, first third 8.5 s, last third 14.6 s (~39 ms per INSERT).
// Expected: the last 300 statements run about as fast as the first 300.
// Actual: they take much longer (and the whole script far more than the database needs).
// Code: apps/desktop/src/components/QueryTab.tsx run(): one result per statement in `own`,
// update()/setResults over all of them for each event; ResultPanel renders every tab.
import assert from "node:assert/strict";
import { button, openEditor, resultTabs, runStatement, saveConnections, visibleTab } from "../editor-helpers.mjs";
import { CTRL } from "../cdp.mjs";

export const setup = ({ dir }) => saveConnections(dir, [], { sqliteSql: "CREATE TABLE seed (id INTEGER);" });

const N = 900;
const finished = `[...${visibleTab}.querySelectorAll('.result-tab')].filter(t => /affected|Error/.test(t.textContent)).length`;

export default async function (page) {
  await openEditor(page, "QA SQLite");
  await runStatement(page, "CREATE TEMP TABLE t (id INTEGER, v TEXT)");
  // The editor draws only the lines in view: the script is in once its last line shows at the end.
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(Array.from({ length: N }, (_, i) => `INSERT INTO t VALUES (${i}, 'v${i}');`).join("\n"));
  await page.key("End", "End", 35, CTRL);
  await page.waitFor(`[...${visibleTab}.querySelector('.cm-content').querySelectorAll('.cm-line')].at(-1)?.textContent === "INSERT INTO t VALUES (${N - 1}, 'v${N - 1}');"`, 30_000);
  await page.key("Escape", "Escape", 27);
  const start = Date.now();
  await page.eval(`${button("Script")}.click()`);
  const at = {};
  for (const mark of [N / 3, (2 * N) / 3, N]) {
    await page.waitFor(`${finished} >= ${mark}`, 600_000);
    at[mark] = Date.now() - start;
  }
  assert.equal((await page.eval(resultTabs)).length, N);
  const first = at[N / 3];
  const last = at[N] - at[(2 * N) / 3];
  console.log(`     ${N} statements in ${at[N]} ms: first third ${first} ms, last third ${last} ms`);
  assert.ok(last < first * 1.5, `the last third took ${last} ms against ${first} ms for the first`);
  assert.ok(at[N] < 30_000, `${N} in-memory INSERTs took ${at[N]} ms`);
}
