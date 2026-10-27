// SQL-06. The error marker and the cursor land before the wrong word when the statement has
// characters outside the BMP (emoji) before the error: servers report the position in
// characters (code points), the editor counts UTF-16 units. Same on PostgreSQL, MySQL,
// ClickHouse and DuckDB, whose drivers also report code points.
// Steps: SQLite, Ctrl+Enter on `SELECT '😀😀', nosuch FROM t`; type "Z" where the cursor went.
// Expected: the cursor is at `nosuch` (text becomes `..., Znosuch FROM t`).
// Actual: it lands one unit earlier per emoji: `SELECT '😀😀'Z, nosuch FROM t`.
// Code: apps/desktop/src/components/QueryTab.tsx showError() uses `position` (DbError,
// "1-based character offset") as a UTF-16 index.
import assert from "node:assert/strict";
import { editorText, openEditor, runStatement, saveConnections, visibleTab } from "../qa-sql.mjs";

export const setup = ({ dir }) => saveConnections(dir, [], { sqliteSql: "CREATE TABLE t (id INTEGER);" });

export default async function (page) {
  await openEditor(page, "QA SQLite");
  assert.equal(await runStatement(page, "SELECT '😀😀', nosuch FROM t"), "Error");
  await page.waitFor(`document.activeElement === ${visibleTab}.querySelector('.cm-content')`);
  await page.type("Z");
  await page.waitFor(`${editorText}.includes('Z')`);
  assert.equal(await page.eval(editorText), "SELECT '😀😀', Znosuch FROM t");
}
