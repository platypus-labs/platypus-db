// SQL on a result: the field above the grid runs a query, or a clause, on a copy of the loaded
// rows in an in-memory DuckDB and the grid shows its output in place of the result; Escape
// returns to the result. After the statement runs again, the next query loads the new rows.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { openTable, rerun, sqliteConnection, status, visibleTab } from "../query.mjs";

const NAME = "E2E sql on results";
const field = `${visibleTab}?.querySelector('.result-sql .cm-content')`;
const fieldText = `(${visibleTab}?.querySelector('.result-sql .cm-line')?.textContent ?? '')`;
const mark = `(${visibleTab}?.querySelector('.result-sql-mark')?.textContent ?? '')`;
const completing = `${visibleTab}?.querySelector('.result-sql .cm-tooltip-autocomplete') != null`;
const clearStatus = `(() => { const s = ${visibleTab}?.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "results.db",
    id: "00000000-0000-4000-8000-000000000033",
    name: NAME,
    sql: `CREATE TABLE items (id INTEGER PRIMARY KEY, grp INTEGER, name TEXT);
           INSERT INTO items VALUES (1, 1, 'c'), (2, 2, 'b'), (3, 1, 'a'), (4, 2, 'd'), (5, 1, 'e');`,
  });
}

/** Types `sql` as the field's whole text. It ends with a character that closes the completion
 *  list, which would otherwise take Enter. */
async function typeInField(page, sql) {
  await page.waitFor(`${field} != null`);
  await page.eval(`${field}.focus()`);
  await page.key("a", "KeyA", 65, 2);
  await page.type(sql);
  await page.waitFor(`${fieldText} === ${JSON.stringify(sql)}`);
  await page.waitFor(`!(${completing})`);
}

/** Presses `key` in the field and waits for the status to become `rows` (emptied first: the
 *  old text may be the same). */
async function pressIn(page, key, code, keyCode, rows) {
  await page.eval(clearStatus);
  await page.eval(`${field}.focus()`);
  await page.key(key, code, keyCode);
  await page.waitFor(`${status} === ${JSON.stringify(rows)}`, 15_000);
}

export default async function (page, { dir }) {
  await openTable(page, NAME, "items", "5 rows");

  // A whole query on the copy of the rows: the grid shows its output.
  await typeInField(page, "SELECT grp, count(*) AS n FROM result GROUP BY grp ORDER BY grp;");
  await pressIn(page, "Enter", "Enter", 13, "2 rows");
  await page.waitFor(`${mark}.startsWith('SQL view · 2 rows')`);

  // Escape returns to the result.
  await pressIn(page, "Escape", "Escape", 27, "5 rows");
  assert.equal(await page.eval(mark), "");
  assert.equal(await page.eval(fieldText), "");

  // A condition alone is the WHERE of the result.
  await typeInField(page, "grp = 1");
  await pressIn(page, "Enter", "Enter", 13, "3 rows");

  // The statement runs again on a new row: the grid shows the result, and the next query
  // loads the new rows.
  const db = new DatabaseSync(join(dir, "results.db"), { timeout: 5_000 });
  db.exec("INSERT INTO items VALUES (6, 1, 'f')");
  db.close();
  await rerun(page);
  assert.equal(await page.eval(status), "6 rows");
  assert.equal(await page.eval(mark), "");
  assert.equal(await page.eval(fieldText), "grp = 1");
  await pressIn(page, "Enter", "Enter", 13, "4 rows");
  await page.waitFor(`${mark}.includes('reloaded')`);
  await pressIn(page, "Escape", "Escape", 27, "6 rows");
}
