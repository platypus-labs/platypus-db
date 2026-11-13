// CONN-13 (low). A port out of range (70000) is sent by Test connection, which skips the
// form's validation, and the answer is the raw deserialization error of the command:
// "invalid args `connection` for command `test_connection`: invalid value: integer `70000`,
// expected u16". Port 0 is accepted and fails with an OS error.
// Steps: New connection, PostgreSQL; Port 70000; Test connection.
// Expected: a plain message about the port (1–65535), as the field's own validation would
// give on Save. Actual: the raw serde/IPC message.
// Suspected: apps/desktop/src/components/ConnectionDialog.tsx `test()` (no validation).
import assert from "node:assert/strict";
import { fillForm, newConnection, seedSqlite, testConnection } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newConnection(page, "PostgreSQL");
  await fillForm(page, { Host: "localhost", Port: 70000, Database: "app", User: "me" });
  const message = await testConnection(page);
  assert.doesNotMatch(message, /invalid args|expected u16|test_connection/, message);
  assert.match(message, /port/i, message);
}
