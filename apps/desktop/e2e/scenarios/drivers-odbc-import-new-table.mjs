// DBS-22 (medium): through the generic ODBC driver, "Import data into a new table" fails on SQL
// Server for any file with a decimal or timestamp column: the new table is declared with the
// ANSI type names DuckDB inferred (BIGINT, VARCHAR, DOUBLE, DATE, TIMESTAMP), and Transact-SQL
// has no DOUBLE (and its TIMESTAMP is a row version), so CREATE TABLE fails: "Nothing was
// imported: Incorrect syntax near ','". The type boxes can be edited by hand, but nothing
// tells the user which names the server takes.
//
// Steps: SQL Server (docker profile sqlserver) through ODBC with Windows' "SQL Server" driver;
// navigator › schema qa_dbs › Import data into a new table…, a CSV "id,name,amount,day,ts"
// with values 1,x,1.25,2025-01-02,2025-01-02 10:00:00; table name qa_dbs_imp; Import.
// Expected: the table created with types the server knows (the driver's SQLGetTypeInfo names:
// float, datetime2…), "Imported 1 rows".
// Actual: "Nothing was imported: Incorrect syntax near ','."
// Suspected: the target types of a new table for DriverKind::Odbc (crates/platypus-transfer
// import planning, `declared_type` / the ODBC dialect's type names), not taken from the
// driver's type info.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { MSSQL_DROP, mssqlSchema, needsPorts, odbcConnection, openPath, sh } from "../drivers-helpers.mjs";
import { chooseFile, closeModal, fill, importDialog, menu, runJob } from "../transfer-helpers.mjs";

export const skip = needsPorts(51433);

export function setup({ dir }) {
  writeFileSync(join(dir, "in.csv"), "id,name,amount,day,ts\n1,x,1.25,2025-01-02,2025-01-02 10:00:00\n");
  mssqlSchema(`CREATE TABLE qa_dbs.anchor (id int);`);
  odbcConnection(dir);
}

export default async function (page, { dir }) {
  try {
    await openPath(page, ["QA ODBC", "qa_dbs", "Tables", "anchor"]);
    await menu(page, "qa_dbs", "Import data into a new table", importDialog, ["QA ODBC"]);
    assert.equal(await chooseFile(page, join(dir, "in.csv")), null);
    await page.eval(fill(`${importDialog}.querySelector('label.field:not(.grow) input')`, "qa_dbs_imp"));
    const out = await runJob(page, importDialog, "Import", 60_000);
    await closeModal(page, importDialog);
    assert.match(out, /Imported 1 row/, out);
    assert.equal(sh("mssql", "SET NOCOUNT ON; SELECT count(*) FROM qa_dbs.qa_dbs_imp"), "1");
  } finally {
    sh("mssql", MSSQL_DROP);
  }
}
