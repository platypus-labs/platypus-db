// SQL-10. ClickHouse: a parameter value with backslashes goes in unescaped, but ClickHouse
// reads `\` as an escape in strings: `C:\temp\new` arrives as "C:<tab>emp<newline>ew" (wrong
// value, length 9), and a value ending with `\` breaks the statement ("Single quoted string is
// not closed"). Only the MySQL driver doubles backslashes.
// Steps: ClickHouse, Ctrl+Enter on `SELECT :v AS v, length(:v) AS len`, answer `C:\temp\new`.
// Expected: v = C:\temp\new, len = 11.
// Actual: len = 9, the value holds a tab and a newline.
// Code: apps/desktop/src/editor/params.ts literal(): `driver === "mysql"` only.
import assert from "node:assert/strict";
import { DB, answerParams, clearStatus, gridTsv, openEditor, saveConnections, setEditor, skipUnless, waitDone } from "../editor-helpers.mjs";
import { CTRL } from "../cdp.mjs";

export const skip = () => skipUnless("clickhouse");
export const setup = ({ dir }) => saveConnections(dir, ["clickhouse"]);

export default async function (page) {
  await openEditor(page, DB.clickhouse.name);
  await setEditor(page, "SELECT length(:v) AS len");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await answerParams(page, ["C:\\temp\\new"]);
  assert.equal(await waitDone(page), "1 row");
  assert.equal(await gridTsv(page), "len\n11");
}
