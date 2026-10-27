// SQL-28. SQL Server: MERGE cannot be run from the editor. The statement is sent without its
// final `;` (spans exclude the delimiter), and SQL Server requires MERGE to end with one:
// "10713: A MERGE statement must be terminated by a semi-colon (;)". Ctrl+Enter and the script
// both fail, even when the `;` is typed. (Oracle PL/SQL keeps its `;` through is_plsql.)
// Steps: SQL Server, Ctrl+Enter on the MERGE below (ending with `;`).
// Expected: 1 row affected.
// Actual: error 10713.
// Code: crates/platypus-sql/src/lib.rs split() drops the delimiter; nothing puts it back for
// T-SQL statements that need it (MERGE, and a WITH after another statement in a batch).
import assert from "node:assert/strict";
import { DB, openEditor, resultError, runScript, runStatement, saveConnections, skipUnless } from "../qa-sql.mjs";

export const skip = () => skipUnless("mssql");
export const setup = ({ dir }) => saveConnections(dir, ["mssql"]);

export default async function (page) {
  await openEditor(page, DB.mssql.name);
  await runScript(page, "IF OBJECT_ID('dbo.qa_sql_merge') IS NOT NULL DROP TABLE dbo.qa_sql_merge\nGO\nCREATE TABLE dbo.qa_sql_merge (id int)");
  const status = await runStatement(
    page,
    "MERGE INTO dbo.qa_sql_merge AS t USING (SELECT 1 AS id) AS s ON t.id = s.id WHEN NOT MATCHED THEN INSERT (id) VALUES (s.id);",
  );
  const error = await page.eval(resultError);
  await runStatement(page, "DROP TABLE dbo.qa_sql_merge");
  assert.equal(error, null);
  assert.equal(status, "1 row affected");
}
