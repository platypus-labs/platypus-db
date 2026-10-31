// qa4-sql-7 (low). Selecting two statements and pressing Ctrl+Enter on SQLite (and ClickHouse)
// shows the driver's raw refusal "Multiple statements provided" / "Syntax error (Multi-statements
// are not allowed)" (ClickHouse, with a character position into the selection). Nothing says
// that the selection can be run as a script (Alt+X), which is what the person meant.
// Steps: SQLite editor, `SELECT 1 AS a;` newline `SELECT 2 AS b`, select all, Ctrl+Enter.
// Expected: the selection runs as a script (two result tabs), or the message tells to use
//   Run script (Alt+X).
// Actual: "Multiple statements provided (SQLite 3.53.2)".
// Code: the selection path of QueryTab.tsx passes the selection to run_statement unchanged; the
//   drivers' own errors are shown as they come.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { clearStatus, openEditor, resultError, resultTabs, saveConnections, setEditor, waitDone } from "../qa-sql.mjs";

export const setup = ({ dir }) => saveConnections(dir, [], { sqliteSql: "CREATE TABLE t (id INTEGER);" });

export default async function (page) {
  await openEditor(page, "QA SQLite");
  await setEditor(page, "SELECT 1 AS a;" + String.fromCharCode(10) + "SELECT 2 AS b");
  await page.key("a", "KeyA", 65, CTRL);
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await waitDone(page);
  const tabs = await page.eval(resultTabs);
  const err = (await page.eval(resultError)) ?? "";
  assert.ok(tabs.length === 2 || /script|Alt\+X/i.test(err), `two results, or a message that points to Run script; got ${tabs.length} tab(s), "${err}"`);
}
