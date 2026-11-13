// CONN-14 (low). Spaces around the host (easy to get by copying it from a document) are kept:
// " localhost " fails with "error connecting to server: unknown host (os error 11001)" in the
// system's language, with nothing pointing at the spaces.
// Steps: New connection, PostgreSQL; Host " localhost " and the container's port, database,
// user and password; Test connection.
// Expected: connected (the host trimmed, as the name is). Actual: unknown host.
// Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { fillForm, needsPort, newConnection, seedSqlite, testConnection } from "../conn-helpers.mjs";

export const skip = needsPort(55432, "PostgreSQL");

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newConnection(page, "PostgreSQL");
  await fillForm(page, { Host: " localhost ", Port: 55432, Database: "platypus", User: "platypus", Password: "platypus" });
  assert.match(await testConnection(page), /^Connected: PostgreSQL/);
}
