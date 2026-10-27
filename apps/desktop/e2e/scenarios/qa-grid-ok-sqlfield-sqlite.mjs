// Regression guard (passes): the SQL field above a result grid runs a condition, a clause and
// a GROUP BY by name on the loaded rows, Escape returns to the result, and the larger editor
// opens the query in a tab of its own.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, assert, connections, copyAll, editBar, editorText, expand, openData, status, stubClipboard, visibleTab } from "../qa-grid.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "field.db"));
  db.exec(`CREATE TABLE t (id INTEGER PRIMARY KEY, city TEXT, note TEXT);
           INSERT INTO t VALUES (1, 'Rome', 'it''s'), (2, 'Oslo', NULL), (3, 'Rome', 'a,b'), (4, 'Rome', '');`);
  db.close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "field.db") }]]);
}

const field = `${visibleTab}.querySelector('.result-sql-field .cm-content')`;
// Each run below shows another row count, which the mark names: the signal that it ended.
const mark = `${visibleTab}.querySelector('.result-sql-mark')`;

async function fieldRun(page, text) {
  await page.eval(`${field}.focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(text);
  await page.waitFor(`${field}.textContent === ${JSON.stringify(text)}`);
  await page.key("Escape", "Escape", 27); // a completion list, if one opened
  await page.eval(`${field}.focus(); window.__mark = ${mark}?.textContent ?? null`);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${mark} !== null && ${mark}.textContent !== window.__mark`, 15_000);
  return copyAll(page);
}

export default async function (page) {
  await expand(page, "QA LITE", null, "t");
  await openData(page, "t");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await stubClipboard(page);
  assert.equal(await fieldRun(page, "note LIKE '%''%' OR note IS NULL"), "1\tRome\tit's\n2\tOslo\t");
  assert.equal(await fieldRun(page, "ORDER BY id DESC LIMIT 3"), "4\tRome\t\n3\tRome\ta,b\n2\tOslo\t");
  assert.equal(await fieldRun(page, "GROUP BY city ORDER BY city"), "Oslo\t1\nRome\t3");
  // Escape goes back to the result.
  await page.eval(`${field}.focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${mark} === null`);
  assert.equal(await page.eval(status), "4 rows");
  assert.equal((await copyAll(page)).split("\n").length, 4);
  // The larger editor opens the query in a tab.
  await page.eval(`${field}.focus()`);
  await page.type("id > 2");
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`document.querySelector('.result-sql-popover') !== null`);
  await page.eval(`[...document.querySelectorAll('.result-sql-popover button')].find(b => b.textContent === 'Open in a tab').click()`);
  await page.waitFor(`[...document.querySelectorAll('.tab')].some(t => t.textContent.startsWith('SQL on results'))`, 15_000);
  await page.waitFor(`${editorText} === 'SELECT * FROM result WHERE id > 2'`);
}
