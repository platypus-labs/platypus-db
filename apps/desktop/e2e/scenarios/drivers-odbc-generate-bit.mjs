// DBS-05 (high): through the generic ODBC driver, "Generate data…" adds nothing to a SQL
// Server table with a BIT column: the "Yes / no" generator writes TRUE / FALSE, which
// Transact-SQL reads as column names ("Invalid column name 'TRUE'"), and the whole run fails.
//
// Steps: SQL Server (docker profile sqlserver) through ODBC with Windows' "SQL Server" driver;
// qa_dbs.g (id int PRIMARY KEY, ok bit, name nvarchar(20)); navigator › g › Generate data…,
// keep the suggestions (Next key, Yes / no, Full name), 5 rows.
// Expected: "Added 5 rows." (1 / 0 for a bit, as the native SQL Server driver writes).
// Actual: "Nothing was added: Invalid column name 'TRUE'."
// Suspected: the boolean literal of the generic ODBC dialect in crates/platypus-sql (the
// insert of generated rows), which assumes TRUE/FALSE are valid.
import assert from "node:assert/strict";
import { MSSQL_DROP, mssqlSchema, needsPorts, odbcConnection, openPath, sh } from "../drivers-helpers.mjs";
import { generate } from "../transfer-helpers.mjs";

export const skip = needsPorts(51433);

export function setup({ dir }) {
  mssqlSchema(`CREATE TABLE qa_dbs.g (id int PRIMARY KEY, ok bit, name nvarchar(20));`);
  odbcConnection(dir);
}

export default async function (page) {
  try {
    await openPath(page, ["QA ODBC", "qa_dbs", "Tables", "g"]);
    const { kinds, out } = await generate(page, "g", ["QA ODBC", "qa_dbs"], 5);
    assert.deepEqual(kinds, ["next_key", "bool", "full_name"]);
    assert.match(out, /Added 5 rows/);
    assert.equal(sh("mssql", "SET NOCOUNT ON; SELECT count(*) FROM qa_dbs.g"), "5");
  } finally {
    sh("mssql", MSSQL_DROP);
  }
}
