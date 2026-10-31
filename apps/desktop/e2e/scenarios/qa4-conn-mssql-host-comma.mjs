// QA4-CONN-3 (low). SQL Server people write the address as "host,port" (SSMS, ADO.NET
// `Server=tcp:db,1433`). In the form's Host field "localhost,51433" is not refused as the other
// drivers' "localhost:5432" is ("the host is not valid: it holds … a `:`"): it is tried as a
// name and the answer is "localhost,51433:1433: An error occured during the attempt of
// performing I/O: Host sconosciuto. (os error 11001)".
// Steps: New connection, SQL Server; Host "localhost,51433", sa / Platypus-2026; Test connection.
// Expected: the connection works (host and port split) or the message says the host is not
// valid and why. Actual: the raw I/O error naming "localhost,51433:1433".
// Code: the host validation of the form/backend (it knows `:` but not `,`); crates/platypus-core
// config host check; driver-mssql builds "host:port".
// Skipped when SQL Server is not running.
import assert from "node:assert/strict";
import { fillForm, needsPort, newConnection, seedSqlite, testConnection } from "../conn-helpers.mjs";

export const skip = needsPort(51433, "SQL Server");

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newConnection(page, "SQL Server");
  await fillForm(page, { Host: "localhost,51433", User: "sa", Password: "Platypus-2026" });
  const message = await testConnection(page);
  assert.match(message, /^Connected|host is not valid|not valid/, `the "host,port" form was tried as a name: ${message}`);
}
