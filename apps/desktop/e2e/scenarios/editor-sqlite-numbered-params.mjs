// qa4-sql-9 (low). SQLite's numbered parameters `?1`, `?2` (the same number may be used twice)
// are taken for the plain `?` placeholder followed by a digit: `SELECT ?1 AS x, ?2 AS y, ?1 AS z`
// opens a dialog with THREE fields (?1, ?2, ?3), and the values are pasted over the `?` only, so
// the statement becomes `SELECT 11 AS x ...` and fails with `near "1": syntax error`.
// Steps: SQLite editor, Ctrl+Enter on `SELECT ?1 AS x, ?2 AS y, ?1 AS z`.
// Expected: the dialog asks two values (?1, ?2) and the row is x=1, y=2, z=1 for the values 1 and 2.
// Actual: three fields, then a syntax error.
// Code: crates/platypus-sql/src/params.rs parameters() (`?` followed by digits) and the
//   substitution that uses the parameter's span.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { answerParams, clearStatus, gridTsv, openEditor, resultError, saveConnections, setEditor, waitDone } from "../editor-helpers.mjs";

export const setup = ({ dir }) => saveConnections(dir, [], { sqliteSql: "CREATE TABLE t (id INTEGER);" });

export default async function (page) {
  await openEditor(page, "QA SQLite");
  await setEditor(page, "SELECT ?1 AS x, ?2 AS y, ?1 AS z");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`document.querySelector('.params-dialog')?.querySelector('input') != null`);
  const fields = await page.eval(`document.querySelectorAll('.params-dialog input').length`);
  await answerParams(page, ["1", "2", "3"].slice(0, fields));
  await waitDone(page);
  assert.equal(await page.eval(resultError), null, `${fields} fields asked; the statement fails`);
  assert.equal(await gridTsv(page), "x\ty\tz\n1\t2\t1");
  assert.equal(fields, 2, "?1 and ?2 are two parameters");
}
