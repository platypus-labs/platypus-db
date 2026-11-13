// QA4-CONN-9 (medium). Explorer's "Copy as path" (and Shift+right-click) puts the path in double
// quotes, and so do many terminals' drag and drop. Pasted in the SQLite (or DuckDB) "Database
// file" field, "D:\data\app.db" is not accepted: Test connection says `"D:\data\app.db" not
// found: a relative path is not created, give the full path of the file` — about a path that
// is full and exists. Trailing spaces are trimmed (the test passes), a leading space is not:
// ` D:\data\app.db not found: a relative path…`.
// Steps: New connection, SQLite; paste the quoted path of an existing database file; Test.
// Expected: "Connected" (the quotes, like the blanks around, are dropped).
// Actual: the "relative path" error. DuckDB: `"D:\…` does not exist.
// Code: apps/desktop/src/components/ConnectionDialog.tsx (file field) or the sqlite/duckdb
// driver's path check (crates/platypus-driver-sqlite).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fillForm, newConnection, seedSqlite, testConnection } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
  const db = new DatabaseSync(join(dir, "real.db"));
  db.exec("CREATE TABLE t (x)");
  db.close();
}

export default async function (page, ctx) {
  const real = join(ctx.dir, "real.db");
  const failures = [];
  for (const [label, text] of [["double quotes", `"${real}"`], ["leading space", ` ${real}`]]) {
    await newConnection(page, "SQLite");
    await fillForm(page, { "Database file": text });
    const message = await testConnection(page);
    if (!/^Connected/.test(message)) failures.push(`${label}: ${message}`);
    await page.eval(`[...document.querySelectorAll('.connection-dialog button')].find(b => b.textContent.trim() === 'Cancel').click()`);
    await page.waitFor(`document.querySelector('.connection-dialog') === null && document.querySelector('.modal.driver-picker') === null`);
  }
  assert.deepEqual(failures, [], failures.join(" | "));
}
