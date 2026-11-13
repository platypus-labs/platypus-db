// DBS-02 (critical, data corruption): through the generic ODBC driver, a value saved from the
// data editor into a SQL Server NVARCHAR column loses every character outside the server's
// code page: 'nëw 😀 Ж' is stored as 'nëw ?? ?'. The editor writes plain string literals
// ('…') where SQL Server needs N'…' (or bound parameters), so the server converts the text to
// its VARCHAR code page first. The grid then shows the damaged value as saved. "Import data"
// of a UTF-8 CSV into the same table damages the text the same way ('héllo 😀 Ж' → 'héllo ?? ?').
//
// Steps: SQL Server (docker profile sqlserver) reached through ODBC with Windows' "SQL Server"
// driver; table qa_dbs.t (id int PRIMARY KEY, name nvarchar(40)); Open data, edit name of
// row 1 to 'nëw 😀 Ж', Save.
// Expected: the column holds 'nëw 😀 Ж' (as with the native SQL Server driver, or DBeaver
// over ODBC/JDBC, which bind parameters).
// Actual: it holds 'nëw ?? ?'; Preview SQL shows SET "name" = 'nëw 😀 Ж' without N.
// Suspected: the generic ODBC dialect's string literal in crates/platypus-sql (dml / literal
// writing for Dialect of DriverKind::Odbc), used by the grid's save.
import assert from "node:assert/strict";
import { editBar, focusCell, MSSQL_DROP, mssqlSchema, needsPorts, odbcConnection, openData, openPath, previewSql, save, setCell, sh } from "../drivers-helpers.mjs";

export const skip = needsPorts(51433);

const VALUE = "nëw 😀 Ж";

export function setup({ dir }) {
  mssqlSchema(`CREATE TABLE qa_dbs.t (id int PRIMARY KEY, name nvarchar(40));
INSERT INTO qa_dbs.t VALUES (1, N'old');`);
  odbcConnection(dir);
}

export default async function (page) {
  try {
    await openPath(page, ["QA ODBC", "qa_dbs", "Tables", "t"]);
    await openData(page, "t");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
    await focusCell(page, 0, 1);
    await setCell(page, VALUE);
    assert.match(await previewSql(page), /UPDATE/);
    assert.match(await save(page), /to edit a cell/);
    assert.equal(sh("mssql", "SET NOCOUNT ON; SELECT name FROM qa_dbs.t WHERE id = 1"), VALUE);
  } finally {
    sh("mssql", MSSQL_DROP);
  }
}
