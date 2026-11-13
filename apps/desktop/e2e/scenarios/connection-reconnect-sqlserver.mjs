// CONN-02 (medium), SQL Server. A session killed on the server (KILL from another session)
// makes the next statement fail with "An error occured during the attempt of performing
// I/O: …" twice in the message; only the statement after it reconnects. PostgreSQL
// reconnects on the first one.
// Steps: two SQL Server connections; the victim stores its @@SPID in qa_conn_spid; the other
// one KILLs it; the victim runs `SELECT 1`.
// Expected: 1 row (the session is reopened). Actual: Error; the next run works.
// Skipped when SQL Server is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { needsPort, openQueryTab, run, visibleTab } from "../conn-helpers.mjs";

export const skip = needsPort(51433, "SQL Server");

const connection = (id, name) => `
[[connections]]
id = "${id}"
name = "${name}"
driver = "sqlserver"
host = "localhost"
port = 51433
database = "platypus"
user = "sa"
save_password = false
password_command = "echo Platypus-2026"
`;

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7
${connection("00000000-0000-4000-8000-0000000000e7", "QA ms victim")}
${connection("00000000-0000-4000-8000-0000000000e9", "QA ms killer")}`,
  );
}

const tab = (name) => `[...document.querySelectorAll('.tab')].find(t => t.textContent.startsWith(${JSON.stringify(name)}))`;

export default async function (page) {
  await openQueryTab(page, "QA ms victim");
  await run(page, "IF OBJECT_ID('dbo.qa_conn_spid') IS NULL CREATE TABLE dbo.qa_conn_spid (spid INT)");
  await run(page, "DELETE FROM dbo.qa_conn_spid");
  assert.equal(await run(page, "INSERT INTO dbo.qa_conn_spid VALUES (@@SPID)"), "1 row affected");

  await openQueryTab(page, "QA ms killer");
  await run(page, "DECLARE @s NVARCHAR(20) = (SELECT TOP 1 CAST(spid AS NVARCHAR(20)) FROM dbo.qa_conn_spid) EXEC('KILL ' + @s)");

  await page.eval(`${tab("QA ms victim")}.click()`);
  await page.waitFor(`${visibleTab}.innerText.includes('QA ms victim')`);
  // Only a session idle over a second is asked whether the server still holds it
  // (IDLE_CHECK in src-tauri/src/commands/mod.rs): statements that follow one another pay no
  // round trip. The kill above can come sooner than that after the victim's INSERT.
  await page.sleep(1500);
  let status;
  let text;
  try {
    status = await run(page, "SELECT 1 AS one");
    text = await page.eval(`${visibleTab}.innerText.split('\\n').slice(-4).join(' | ')`);
  } finally {
    // Cleanup, again if the drop met the dropped session.
    if ((await run(page, "DROP TABLE IF EXISTS dbo.qa_conn_spid")) === "Error") await run(page, "DROP TABLE IF EXISTS dbo.qa_conn_spid");
  }
  assert.equal(status, "1 row", `the first statement after the session was killed failed: ${text}`);
}
