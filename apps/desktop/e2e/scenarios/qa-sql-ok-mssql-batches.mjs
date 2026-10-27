// Regression guard (passes): SQL Server batches. GO splits batches and a procedure body runs
// to its GO; BEGIN TRY / BEGIN CATCH and BEGIN TRAN keep their statements apart as they
// should; temp tables live across statements of the tab; Cancel stops a WAITFOR at once and
// the session stays usable.
import assert from "node:assert/strict";
import { DB, button, clearStatus, gridTsv, openEditor, resultError, resultTabs, runScript, runStatement, saveConnections, setEditor, skipUnless, waitDone } from "../qa-sql.mjs";
import { CTRL } from "../cdp.mjs";

export const skip = () => skipUnless("mssql");
export const setup = ({ dir }) => saveConnections(dir, ["mssql"]);

export default async function (page) {
  await openEditor(page, DB.mssql.name);
  await runScript(
    page,
    "IF OBJECT_ID('dbo.qa_sql_ok_p', 'P') IS NOT NULL DROP PROCEDURE dbo.qa_sql_ok_p\nGO\nCREATE PROCEDURE dbo.qa_sql_ok_p @a int AS\nBEGIN\n  SET NOCOUNT ON;\n  SELECT @a + 1 AS r;\nEND\nGO\nEXEC dbo.qa_sql_ok_p 41;",
  );
  assert.equal(await page.eval(resultError), null);
  assert.equal((await page.eval(resultTabs)).length, 3);
  assert.equal(await gridTsv(page), "r\n42");

  await runScript(page, "BEGIN TRAN;\nSELECT @@TRANCOUNT AS tc;\nROLLBACK;");
  assert.deepEqual(await page.eval(resultTabs), ["1. 0 rows affected", "2. 1 row", "3. 0 rows affected"]);

  await runScript(page, "SELECT 1 AS x INTO #qa_sql_ok_t;\nSELECT * FROM #qa_sql_ok_t;");
  assert.equal(await gridTsv(page), "x\n1");

  await setEditor(page, "WAITFOR DELAY '00:00:20'");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${button("Cancel")}?.disabled === false`, 10_000);
  await page.sleep(1000);
  const started = Date.now();
  await page.eval(`${button("Cancel")}.click()`);
  assert.equal(await waitDone(page), "Cancelled");
  assert.ok(Date.now() - started < 5000);
  assert.equal(await runStatement(page, "SELECT 1 AS ok"), "1 row");
  await runStatement(page, "DROP PROCEDURE dbo.qa_sql_ok_p");
}
