// XFER-21 (medium). "Copy to a database" creates the new table without its key and NOT NULL.
// Steps: a SQLite table items (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT 'x');
// navigator › items › "Copy to a database…" (the only connection: a copy next to it,
// items_copy), Copy.
// Expected: items_copy has the primary key and NOT NULL (and the default) — as "Copy tables to
// a database…" (migrate) does, and DBeaver's data transfer.
// Actual: CREATE TABLE items_copy (id INTEGER, name TEXT): no key, no NOT NULL, no default.
// Copied into PostgreSQL → MySQL it is the same: `id int DEFAULT NULL`, no PRIMARY KEY.
// Suspected: apps/desktop/src/transfer/ImportDataDialog.tsx:190-215 (plan() never sets
// primary_key, not_null or default for a new table).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { closeModal, expand, importDialog, menu, runJob, sqliteOnly } from "../transfer-helpers.mjs";

export const setup = ({ dir }) => sqliteOnly(dir, "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT 'x'); INSERT INTO items VALUES (1, 'a');");

export default async function (page, { dir }) {
  await expand(page, "QA SQLite", "main");
  await expand(page, "main", "Tables", "QA SQLite");
  await expand(page, "Tables", "items", ["QA SQLite", "main"]);
  await menu(page, "items", "Copy to a database", importDialog, ["QA SQLite", "main"]);
  await page.waitFor(`${importDialog}.querySelectorAll('.import-preview tbody tr').length === 1`, 20_000);
  assert.match(await runJob(page, importDialog, "Copy"), /Imported 1 row/);
  await closeModal(page, importDialog);
  const db = new DatabaseSync(join(dir, "t.db"));
  const { sql } = db.prepare("SELECT sql FROM sqlite_master WHERE name = 'items_copy'").get();
  db.close();
  assert.match(sql, /PRIMARY KEY/i, sql);
  assert.match(sql, /NOT NULL/i, sql);
}
