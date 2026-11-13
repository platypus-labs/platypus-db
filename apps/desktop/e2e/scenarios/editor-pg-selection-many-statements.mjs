// qa4-sql-6 (medium). Selecting several statements and pressing Ctrl+Enter (run the selection)
// behaves differently on each database. PostgreSQL runs them all on the server but shows only
// the FIRST result: the second SELECT's rows (and its errors, if it fails) are dropped without a
// word. MySQL and SQL Server show one result tab per statement; SQLite and ClickHouse refuse with
// a raw "Multiple statements provided" (see editor-selection-many-statements-message).
// Steps: PostgreSQL editor, text `SELECT 1 AS a;` newline `SELECT 2 AS b`, select all, Ctrl+Enter.
// Expected: two result tabs (like MySQL), or the selection is run as a script, or a clear refusal.
// Actual: one grid with `a`; `b` is never shown.
// Code: the selection path of QueryTab.tsx sends the whole selection as one statement
//   (run_statement) and the PostgreSQL driver keeps only the first result set of the batch.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { DB, clearStatus, openEditor, resultError, resultTabs, saveConnections, setEditor, skipUnless, waitDone } from "../editor-helpers.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

export default async function (page) {
  await openEditor(page, DB.pg.name);
  await setEditor(page, "SELECT 1 AS a;" + String.fromCharCode(10) + "SELECT 2 AS b");
  await page.key("a", "KeyA", 65, CTRL);
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await waitDone(page);
  const tabs = await page.eval(resultTabs);
  const err = await page.eval(resultError);
  assert.ok(tabs.length === 2 || err !== null, `both results are shown (or the selection is refused), got ${tabs.length} tab(s) and no error`);
}
