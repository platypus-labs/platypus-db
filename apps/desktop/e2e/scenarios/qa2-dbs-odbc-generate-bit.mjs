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
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { needsPorts, openPath, sh } from "../qa-dbs.mjs";
import { generate } from "../qa-xfer.mjs";

export const skip = needsPorts(51433);

export function setup({ dir }) {
  sh("mssql", `IF SCHEMA_ID('qa_dbs') IS NOT NULL BEGIN DROP TABLE IF EXISTS qa_dbs.g; DROP SCHEMA qa_dbs; END;
GO
CREATE SCHEMA qa_dbs;
GO
CREATE TABLE qa_dbs.g (id int PRIMARY KEY, ok bit, name nvarchar(20));
GO`);
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000d0082"
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
    await openPath(page, ["QA ODBC", "qa_dbs", "Tables", "g"]);
    const { kinds, out } = await generate(page, "g", ["QA ODBC", "qa_dbs"], 5);
    assert.deepEqual(kinds, ["next_key", "bool", "full_name"]);
    assert.match(out, /Added 5 rows/);
    assert.equal(sh("mssql", "SET NOCOUNT ON; SELECT count(*) FROM qa_dbs.g"), "5");
  } finally {
    sh("mssql", "DROP TABLE IF EXISTS qa_dbs.g; DROP SCHEMA IF EXISTS qa_dbs;");
  }
}
