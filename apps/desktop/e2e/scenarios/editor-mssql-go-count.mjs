// SQL-22 (low). SQL Server: `GO 3` (run the batch three times, as SSMS and sqlcmd do) is
// accepted as a separator but its count is ignored: the batch runs once.
// Steps: SQL Server script: create a temp table, `INSERT ... VALUES (1)` then `GO 3`, count.
// Expected: 3.
// Actual: 1.
// Code: crates/platypus-sql/src/lib.rs batch_separator() reads the count and drops it.
import assert from "node:assert/strict";
import { DB, gridTsv, openEditor, runScript, saveConnections, skipUnless } from "../editor-helpers.mjs";

export const skip = () => skipUnless("mssql");
export const setup = ({ dir }) => saveConnections(dir, ["mssql"]);

export default async function (page) {
  await openEditor(page, DB.mssql.name);
  await runScript(page, "CREATE TABLE #qa_sql_go (n int)\nGO\nINSERT INTO #qa_sql_go VALUES (1)\nGO 3\nSELECT COUNT(*) AS c FROM #qa_sql_go");
  assert.equal(await gridTsv(page), "c\n3");
}
