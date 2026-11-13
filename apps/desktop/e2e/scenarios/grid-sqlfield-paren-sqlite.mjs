// GRID-10 (low), SQL field above the result grid. A condition that starts with a parenthesis,
// "(id = 1 OR id = 2)", is taken for a whole query, because "(" is one of the words that start
// a query (apps/desktop/src/analysis/sqlField.ts, QUERY_START), and runs as typed: "syntax
// error at or near "id"". So does a condition on a column named like a query keyword
// ("values > 1", "show = 1", "table = 'x'").
// Expected: the rows with id 1 or 2. Actual: a syntax error.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { assert, connections, copyAll, editBar, expand, openData, stubClipboard, visibleTab } from "../grid-helpers.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "field.db"));
  db.exec(`CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT);
           INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'c');`);
  db.close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "field.db") }]]);
}

export default async function (page) {
  await expand(page, "QA LITE", null, "t");
  await openData(page, "t");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await stubClipboard(page);
  const field = `${visibleTab}.querySelector('.result-sql-field .cm-content')`;
  const condition = "(id = 1 OR id = 2)";
  await page.eval(`${field}.focus()`);
  await page.type(condition);
  await page.waitFor(`${field}.textContent === ${JSON.stringify(condition)}`);
  await page.key("Escape", "Escape", 27); // a completion list, if one opened
  await page.eval(`${field}.focus()`);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${visibleTab}.querySelector('.result-sql-mark') !== null || ${visibleTab}.querySelector('.result-sql .edit-error') !== null`, 15_000);
  assert.equal(await page.eval(`${visibleTab}.querySelector('.result-sql .edit-error')?.textContent ?? null`), null);
  assert.equal(await copyAll(page), "1\ta\n2\tb");
}
