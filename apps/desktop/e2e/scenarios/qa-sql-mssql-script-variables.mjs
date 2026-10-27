// SQL-03. SQL Server: a script that declares a variable and uses it in the next statement
// fails, because the script is split at every `;` and each piece is sent as its own batch;
// T-SQL variables live for one batch (up to GO). Every T-SQL script with DECLARE breaks.
// Steps: run as a script `DECLARE @x int = 5;` then `SELECT @x * 2 AS dbl;` on SQL Server.
// Expected: one row, dbl = 10 (statements up to GO run as one batch, as in SSMS/sqlcmd).
// Actual: "137: Must declare the scalar variable "@x"."
// Code: crates/platypus-sql/src/lib.rs split() for Dialect::SqlServer; the QueryTab runs
// each span on its own.
import assert from "node:assert/strict";
import { DB, gridTsv, openEditor, resultError, runScript, saveConnections, skipUnless } from "../qa-sql.mjs";

export const skip = () => skipUnless("mssql");
export const setup = ({ dir }) => saveConnections(dir, ["mssql"]);

export default async function (page) {
  await openEditor(page, DB.mssql.name);
  await runScript(page, "DECLARE @x int = 5;\nSELECT @x * 2 AS dbl;");
  assert.equal(await page.eval(resultError), null);
  assert.equal(await gridTsv(page), "dbl\n10");
}
