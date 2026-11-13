// QA4-CONN-2 (medium). Test connection on a SQLite connection whose file is not a database (a
// text file, a Cargo.toml, a photo) answers "Connected: SQLite 3.53.2": SQLite opens the file
// lazily and the test never reads from it. The connection is then saved and the first
// expansion fails ("file is not a database"), or an import/script tries to write into it.
// Steps: New connection, SQLite, Database file = a text file; Test connection.
// Expected: an error ("file is not a database"), as for a directory ("unable to open database
// file"). Actual: "Connected: SQLite …".
// Code: crates/platypus-driver-sqlite connect()/test (needs one read, e.g. `PRAGMA schema_version`
// or `SELECT count(*) FROM sqlite_master`, before reporting the version).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { fillForm, newConnection, seedSqlite, testConnection } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
  writeFileSync(join(dir, "notes.txt"), "this is a text file, not a database\n".repeat(200));
}

export default async function (page, ctx) {
  await newConnection(page, "SQLite");
  await fillForm(page, { "Database file": join(ctx.dir, "notes.txt") });
  const message = await testConnection(page);
  assert.doesNotMatch(message, /^Connected/, `a text file passed as a SQLite database: ${message}`);
}
