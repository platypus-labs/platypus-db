// CONN-03 (medium). SQL Server: Test connection says "· not encrypted" for a connection that
// is encrypted. With SSL mode `require` the session is TLS (sys.dm_exec_connections says
// encrypt_option = TRUE), but the dialog, from ServerInfo.encrypted, reports plaintext, so
// the user cannot trust what the app says about encryption.
// Steps: new SQL Server connection to the container, SSL mode require; Test connection; then
// in a query tab check sys.dm_exec_connections.
// Expected: "Connected: SQL Server … · TLS". Actual: "… · not encrypted".
// Suspected: crates/platypus-driver-mssql/src/lib.rs (connect: `encrypted` read from
// CONNECTIONPROPERTY('encrypt_option'), which is not 'TRUE' on this session).
// Skipped when SQL Server is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { button, dialog, editConnection, needsPort, openQueryTab, run, testConnection } from "../conn-helpers.mjs";

export const skip = needsPort(51433, "SQL Server");

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000000d1"
name = "QA mssql require"
driver = "sqlserver"
host = "localhost"
port = 51433
database = "platypus"
user = "sa"
ssl_mode = "require"
save_password = false
password_command = "echo Platypus-2026"
`,
  );
}

export default async function (page) {
  // The server's view: this session is encrypted.
  await openQueryTab(page, "QA mssql require");
  assert.equal(
    await run(page, "IF (SELECT encrypt_option FROM sys.dm_exec_connections WHERE session_id = @@SPID) = 'TRUE' SELECT 1 AS encrypted ELSE THROW 50000, 'not encrypted', 1;"),
    "1 row",
  );

  // The dialog's view of the same settings.
  await editConnection(page, "QA mssql require");
  const result = await testConnection(page);
  assert.match(result, /^Connected: SQL Server/);
  assert.match(result, /· TLS$/, `the dialog says: ${result}`);
  await page.eval(`${button(dialog, "Cancel")}.click()`);
}
