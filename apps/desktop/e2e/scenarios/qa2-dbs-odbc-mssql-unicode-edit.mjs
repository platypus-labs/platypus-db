// DBS-02 (critical, data corruption): through the generic ODBC driver, a value saved from the
// data editor into a SQL Server NVARCHAR column loses every character outside the server's
// code page: 'nëw 😀 Ж' is stored as 'nëw ?? ?'. The editor writes plain string literals
// ('…') where SQL Server needs N'…' (or bound parameters), so the server converts the text to
// its VARCHAR code page first. The grid then shows the damaged value as saved.
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
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { editBar, focusCell, needsPorts, openData, openPath, previewSql, save, setCell, sh } from "../qa-dbs.mjs";

export const skip = needsPorts(51433);

const VALUE = "nëw 😀 Ж";

export function setup({ dir }) {
  sh("mssql", `IF SCHEMA_ID('qa_dbs') IS NOT NULL BEGIN DROP TABLE IF EXISTS qa_dbs.t; DROP SCHEMA qa_dbs; END;
GO
CREATE SCHEMA qa_dbs;
GO
CREATE TABLE qa_dbs.t (id int PRIMARY KEY, name nvarchar(40));
INSERT INTO qa_dbs.t VALUES (1, N'old');
GO`);
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000d0081"
name = "QA ODBC"
driver = "odbc"
host = ""
database = ""
user = "sa"
save_password = false
password_command = "echo Platypus-2026"

[connections.options]
driver = "SQL Server"
connection_string = "Server=127.0.0.1,51433;Database=platypus"
`,
  );
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
    sh("mssql", "DROP TABLE IF EXISTS qa_dbs.t; DROP SCHEMA IF EXISTS qa_dbs;");
  }
}
