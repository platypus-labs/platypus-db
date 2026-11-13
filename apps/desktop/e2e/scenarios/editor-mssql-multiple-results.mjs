// SQL-02. A statement that returns several result sets shows only the first one; the others
// are read and dropped without a word. On SQL Server this hits every batch with more than one
// SELECT (no `;` between them), procedures returning several results, and TRY/CATCH blocks
// (the CATCH's SELECT ERROR_MESSAGE() is lost, the user sees "0 rows" and no error). MySQL
// `CALL` of a procedure with two SELECTs loses the second the same way.
// Steps: on SQL Server, Ctrl+Enter on `SELECT 1 AS a SELECT 2 AS b`.
// Expected: two results (two result tabs, the second with column b), as SSMS and DBeaver show.
// Actual: one grid with column a only.
// Code: crates/platypus-driver-mssql/src/lib.rs (module doc: "streams the first one and
// reads the rest to the end without returning them"), platypus-driver-mysql drop_result().
import assert from "node:assert/strict";
import { DB, gridTsv, openEditor, resultTabs, runStatement, saveConnections, skipUnless } from "../editor-helpers.mjs";

export const skip = () => skipUnless("mssql");
export const setup = ({ dir }) => saveConnections(dir, ["mssql"]);

export default async function (page) {
  await openEditor(page, DB.mssql.name);
  await runStatement(page, "SELECT 1 AS a SELECT 2 AS b");
  const tabs = await page.eval(resultTabs);
  const shown = await gridTsv(page);
  assert.ok(tabs.length >= 2 || shown.includes("b\n2"), `only the first result is shown: tabs ${JSON.stringify(tabs)}, grid ${JSON.stringify(shown)}`);
}
