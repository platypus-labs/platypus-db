// qa4-sql-4 (low). ClickHouse's ternary operator `cond ? a : b` is taken for a positional
// parameter: Ctrl+Enter on `SELECT 1 ? 'a' : 'b' AS t` opens the Parameters dialog asking for
// "?1" (and, once answered, the value replaces the `?`, so the statement is broken). ClickHouse
// has no `?` placeholders (its parameters are `{name:Type}`).
// Steps: ClickHouse editor, Ctrl+Enter on `SELECT 1 ? 'a' : 'b' AS t`.
// Expected: it runs at once and returns a.
// Actual: a Parameters dialog asks for ?1.
// Code: crates/platypus-sql/src/params.rs parameters(): `?` is accepted for every dialect but
//   PostgreSQL (and the like), including ClickHouse.
import assert from "node:assert/strict";
import { DB, clearStatus, gridTsv, openEditor, saveConnections, setEditor, skipUnless, waitDone } from "../editor-helpers.mjs";
import { CTRL } from "../cdp.mjs";

export const skip = () => skipUnless("clickhouse");
export const setup = ({ dir }) => saveConnections(dir, ["clickhouse"]);

export default async function (page) {
  await openEditor(page, DB.clickhouse.name);
  await setEditor(page, "SELECT 1 ? 'a' : 'b' AS t");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  const asked = await Promise.race([
    page.waitFor(`document.querySelector('.params-dialog') !== null`, 5000).then(() => true),
    waitDone(page, 5000).then(() => false),
  ]);
  assert.ok(!asked, "the ternary operator is not a parameter: no dialog");
  assert.equal(await gridTsv(page), "t\na");
}
