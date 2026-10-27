// SQL-07. After a statement with parameters fails, the error marker and cursor point at the
// wrong place: the server's position is in the SQL with the values filled in, but it is
// applied to the editor's text, where `:p` is shorter or longer than its value.
// Steps: SQLite, Ctrl+Enter on `SELECT :p AS a, nosuch FROM t`, answer a 40-character value;
// type "Z" where the cursor went.
// Expected: `SELECT :p AS a, Znosuch FROM t` (cursor at the unknown column).
// Actual: the cursor lands far to the right (past the end of the statement here).
// Code: apps/desktop/src/components/QueryTab.tsx showError(): `planned.offset + position`
// without mapping back through substitute().
import assert from "node:assert/strict";
import { answerParams, clearStatus, editorText, openEditor, saveConnections, setEditor, visibleTab, waitDone } from "../qa-sql.mjs";
import { CTRL } from "../cdp.mjs";

export const setup = ({ dir }) => saveConnections(dir, [], { sqliteSql: "CREATE TABLE t (id INTEGER);" });

export default async function (page) {
  await openEditor(page, "QA SQLite");
  await setEditor(page, "SELECT :p AS a, nosuch FROM t");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await answerParams(page, ["a much longer value than the placeholder"]);
  assert.equal(await waitDone(page), "Error");
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("Z");
  await page.waitFor(`${editorText}.includes('Z')`);
  assert.equal(await page.eval(editorText), "SELECT :p AS a, Znosuch FROM t");
}
