// SQL-24 (low). MongoDB: the "Change every row?" confirmation for `deleteMany({})` shows the
// statement upper-cased and talks about SQL: "DB.QA_SQL_GUARD.DELETEMANY({}) without WHERE
// changes every row of qa_sql_guard". Collection names are case-sensitive, and mongosh has no
// WHERE.
// Steps: Mongo editor, Ctrl+Enter on `db.qa_sql_guard.deleteMany({})`.
// Expected: the statement as typed and wording for documents and a filter.
// Actual: upper-cased text and "without WHERE".
// Code: apps/desktop/src/guards (check text built from the statement's first words).
import assert from "node:assert/strict";
import { DB, clearStatus, openEditor, runStatement, saveConnections, setEditor, skipUnless } from "../qa-sql.mjs";
import { CTRL } from "../cdp.mjs";

export const skip = () => skipUnless("mongo");
export const setup = ({ dir }) => saveConnections(dir, ["mongo"]);

export default async function (page) {
  await openEditor(page, DB.mongo.name);
  await runStatement(page, "db.qa_sql_guard.insertOne({ a: 1 })");
  await setEditor(page, "db.qa_sql_guard.deleteMany({})");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`document.querySelector('.modal') !== null`, 10_000);
  const text = await page.eval(`document.querySelector('.modal').textContent`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Cancel').click()`);
  await runStatement(page, "db.qa_sql_guard.drop()");
  assert.doesNotMatch(text, /DELETEMANY|WHERE/, text);
}
