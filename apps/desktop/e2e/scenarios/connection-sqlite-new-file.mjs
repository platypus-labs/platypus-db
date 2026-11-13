// CONN-18 (low). A new SQLite database cannot be created from the app: a database file that
// does not exist yet is refused ("… does not exist"), and Browse… only opens existing files.
// DuckDB creates its file when missing; DBeaver creates the SQLite file too.
// Steps: New connection, SQLite; Database file: a new name in an existing folder; Test
// connection.
// Expected: connected, the empty database created (or a "Create" choice). Actual: refused.
// Suspected: crates/platypus-driver-sqlite/src/lib.rs `open` (no SQLITE_OPEN_CREATE).
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { fillForm, newConnection, seedSqlite, testConnection } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page, { dir }) {
  const file = join(dir, "qa_conn_new.db");
  await newConnection(page, "SQLite");
  await fillForm(page, { Name: "QA new file", "Database file": file });
  const message = await testConnection(page);
  assert.match(message, /^Connected: SQLite/, message);
  assert.ok(existsSync(file));
}
