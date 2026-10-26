// SQL on results: "Query with SQL" in the grid's menu copies the rows into the table `result`
// of an in-memory DuckDB, queried in a new tab; after the source tab runs again, "Reload from
// result" copies them again and runs the query once more.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { editorText, openTable, rerun, runQuery, sqliteConnection, status, visibleTab } from "../query.mjs";

const NAME = "E2E sql on results";
const menuButton = (label) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)})`;
const tab = (start) => `[...document.querySelectorAll('.tab')].find(t => t.textContent.startsWith(${JSON.stringify(start)}))`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "results.db",
    id: "00000000-0000-4000-8000-000000000033",
    name: NAME,
    sql: `CREATE TABLE items (id INTEGER PRIMARY KEY, grp INTEGER, name TEXT);
           INSERT INTO items VALUES (1, 1, 'c'), (2, 2, 'b'), (3, 1, 'a'), (4, 2, 'd'), (5, 1, 'e');`,
  });
}

export default async function (page, { dir }) {
  await openTable(page, NAME, "items", "5 rows");

  // From the grid's menu: a new tab on the copied rows.
  const grid = await page.eval(`(() => { const r = ${visibleTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  await page.click(grid.x + 40, grid.y + 52, { button: "right" });
  await page.waitFor(`${menuButton("Query with SQL")} !== undefined`);
  await page.eval(`${menuButton("Query with SQL")}.click()`);
  await page.waitFor(`${tab("SQL on results")}?.classList.contains('active') === true`, 15_000);
  await page.waitFor(`${editorText} === 'SELECT * FROM result'`);
  await rerun(page);
  assert.equal(await page.eval(status), "5 rows");
  await runQuery(page, "SELECT grp, count(*) AS n FROM result GROUP BY grp ORDER BY grp");
  assert.equal(await page.eval(status), "2 rows");

  // A new row read by the source tab, then Reload from result: the query runs again on it.
  const db = new DatabaseSync(join(dir, "results.db"), { timeout: 5_000 });
  db.exec("INSERT INTO items VALUES (6, 3, 'f')");
  db.close();
  await page.eval(`${tab("items")}.click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null`);
  await rerun(page);
  assert.equal(await page.eval(status), "6 rows");
  await page.eval(`${tab("SQL on results")}.click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.result-source') != null`);
  await page.eval(`[...${visibleTab}.querySelectorAll('.result-source button')].find(b => b.textContent === 'Reload from result').click()`);
  await page.waitFor(`${status} === '3 rows'`, 15_000);
}
