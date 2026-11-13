// SQL-23. SQLite also takes `@name` and `$name` parameters, but the editor only asks values
// for `:name` and `?`; the others go to SQLite unbound and silently become NULL, so a
// `WHERE id = @id` returns no rows without any message.
// Steps: SQLite, Ctrl+Enter on `SELECT @c AS c, $d AS d`.
// Expected: the parameters dialog asks for c and d (or the run is refused).
// Actual: no dialog; one row of NULLs.
// Code: crates/platypus-sql/src/params.rs parameters(): only ':' and '?' symbols.
import assert from "node:assert/strict";
import { clearStatus, openEditor, saveConnections, setEditor, waitDone } from "../editor-helpers.mjs";
import { CTRL } from "../cdp.mjs";

export const setup = ({ dir }) => saveConnections(dir, [], { sqliteSql: "CREATE TABLE t (id INTEGER);" });

export default async function (page) {
  await openEditor(page, "QA SQLite");
  await setEditor(page, "SELECT @c AS c, $d AS d");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  const asked = await Promise.race([
    page.waitFor(`document.querySelector('.params-dialog') !== null`, 5000).then(() => true),
    waitDone(page, 5000).then(() => false),
  ]);
  assert.ok(asked, "no parameters dialog: @c and $d ran as NULL");
}
