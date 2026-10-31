// qa4-sql-3 (high: silent wrong result). A parameter filled with a negative number is pasted
// into the text, so `10-:p` with p = -5 becomes `10--5`, and `--` starts a comment: the rest of
// the statement is dropped (here the alias, the other columns and any WHERE/LIMIT after it) and
// the query returns 10 instead of 15, with no error. In PostgreSQL `:p::text` with p = -5
// becomes `-5::text`, which fails ("operator does not exist: - text"); with `(-5)` both work.
// Steps: SQLite editor, Ctrl+Enter on `SELECT 10-:p AS r`, answer the dialog with -5, Run.
// Expected: one column r = 15.
// Actual: one column named `10--5 AS r` with the value 10.
// Code: the parameter substitution that builds the statement text from the dialog's values
//   (a negative number must go in parenthesised or as a bound value): crates/platypus-sql/src/params.rs
//   and the code that calls it in apps/desktop/src-tauri/src/commands/query.rs.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { answerParams, clearStatus, gridTsv, openEditor, saveConnections, setEditor, waitDone } from "../qa-sql.mjs";

export const setup = ({ dir }) => saveConnections(dir, [], { sqliteSql: "CREATE TABLE t (id INTEGER);" });

export default async function (page) {
  await openEditor(page, "QA SQLite");
  await setEditor(page, "SELECT 10-:p AS r");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await answerParams(page, ["-5"]);
  await waitDone(page);
  assert.equal(await gridTsv(page), "r\n15", "10 - (-5) is 15, not a comment");
}
