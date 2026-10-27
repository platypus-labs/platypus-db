// Regression guard (passes): Cancel on SQLite. It stops a long statement at once; in a script
// the statements after the cancelled one are marked "Not executed" and do not run; Ctrl+Enter
// pressed again while a statement runs does not start another; the tab works afterwards.
import assert from "node:assert/strict";
import { button, clearStatus, gridTsv, openEditor, resultTabs, runStatement, saveConnections, setEditor, waitDone } from "../qa-sql.mjs";
import { CTRL } from "../cdp.mjs";

export const setup = ({ dir }) => saveConnections(dir, [], { sqliteSql: "CREATE TABLE t (id INTEGER);" });

const SLOW = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 500000000) SELECT count(*) FROM c";

export default async function (page) {
  await openEditor(page, "QA SQLite");
  await setEditor(page, `SELECT 1 AS a;\n${SLOW};\nINSERT INTO t VALUES (1);`);
  await clearStatus(page);
  await page.eval(`${button("Script")}.click()`);
  await page.waitFor(`${resultTabs}.length === 3 && ${resultTabs}[0].includes('1 row') && ${button("Cancel")}.disabled === false`, 10_000);
  await page.sleep(1000);
  // Ctrl+Enter again while running: ignored.
  await page.key("Enter", "Enter", 13, CTRL);
  const started = Date.now();
  await page.eval(`${button("Cancel")}.click()`);
  await waitDone(page);
  assert.ok(Date.now() - started < 5000);
  assert.deepEqual(await page.eval(resultTabs), ["1. 1 row", "2. Cancelled", "3. Not executed"]);
  assert.equal(await runStatement(page, "SELECT count(*) AS n FROM t"), "1 row");
  assert.equal(await gridTsv(page), "n\n0");
}
