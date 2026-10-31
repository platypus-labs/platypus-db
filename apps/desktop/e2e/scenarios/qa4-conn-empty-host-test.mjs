// QA4-CONN-4 (low). Test connection with the Host field emptied (a new PostgreSQL form starts
// with "localhost"; the user clears it and tests) answers "error performing TLS handshake:
// invalid dns name": a message about TLS for a missing host. Other bad hosts have a clear
// one ("the host is not valid: …").
// Steps: New connection, PostgreSQL; clear Host; port 55432, platypus / platypus; Test connection.
// Skipped when PostgreSQL is not running.
// Expected: a message that the host is missing. Actual: "error performing TLS handshake:
// invalid dns name".
// Code: apps/desktop/src/components/ConnectionDialog.tsx `test()` / platypus-core host check
// (empty string is not validated).
import assert from "node:assert/strict";
import { fillForm, needsPort, newConnection, seedSqlite, testConnection } from "../conn-helpers.mjs";

export const skip = needsPort(55432, "PostgreSQL");

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newConnection(page, "PostgreSQL");
  await fillForm(page, { Port: 55432, Host: "", Database: "platypus", User: "platypus", Password: "platypus" });
  const message = await testConnection(page);
  assert.doesNotMatch(message, /TLS|dns/i, `a missing host reported as a TLS problem: ${message}`);
  assert.match(message, /host/i, message);
}
