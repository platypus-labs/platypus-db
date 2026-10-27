// GRID-09 (medium), SQL field above the result grid (all databases). "GROUP BY 1" groups
// everything into one row: the field writes `SELECT 1, count(*) AS count FROM result GROUP BY
// 1` (apps/desktop/src/analysis/sqlField.ts, selectList copies the GROUP BY expressions into
// the select list), where 1 is the literal one, so the ordinal names that constant: the
// output is one row "1 | 4" instead of the counts of the first column, with no warning.
// "GROUP BY 2" names count(*) and fails: "GROUP BY clause cannot contain aggregates!".
// Steps: open `t` (4 rows, city Rome x3 / Oslo x1), type "GROUP BY 2 ORDER BY 2", Enter.
// Expected: the groups of the 2nd column (Oslo 1, Rome 3). Actual: an error.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { assert, connections, copyAll, editBar, expand, openData, stubClipboard, visibleTab } from "../qa-grid.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "field.db"));
  db.exec(`CREATE TABLE t (id INTEGER PRIMARY KEY, city TEXT);
           INSERT INTO t VALUES (1, 'Rome'), (2, 'Oslo'), (3, 'Rome'), (4, 'Rome');`);
  db.close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "field.db") }]]);
}

export default async function (page) {
  await expand(page, "QA LITE", null, "t");
  await openData(page, "t");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await stubClipboard(page);
  const field = `${visibleTab}.querySelector('.result-sql-field .cm-content')`;
  await page.eval(`${field}.focus()`);
  await page.type("GROUP BY 2 ORDER BY 2");
  await page.waitFor(`${field}.textContent === 'GROUP BY 2 ORDER BY 2'`);
  await page.key("Escape", "Escape", 27); // a completion list, if one opened
  await page.eval(`${field}.focus()`);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${visibleTab}.querySelector('.result-sql-mark') !== null || ${visibleTab}.querySelector('.result-sql .edit-error') !== null`, 15_000);
  assert.equal(await page.eval(`${visibleTab}.querySelector('.result-sql .edit-error')?.textContent ?? null`), null);
  const text = await copyAll(page);
  assert.equal(text, "Oslo\t1\nRome\t3");
}
