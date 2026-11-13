// XFER-12 (medium). A quoted empty CSV field ("") is imported as NULL, not as ''.
// Steps: a CSV `id,note / 1,"" / 2,` (a quoted empty string, then a missing value);
// navigator › main › "Import data into a new table…", Import.
// Expected: row 1 holds the empty string, row 2 NULL — the quotes are how a CSV tells them
// apart (the app's own CSV export cannot, see XFER-23, but other tools write "").
// Actual: both rows hold NULL.
// Suspected: crates/platypus-driver-duckdb/src/files.rs:56-60 (read_csv without
// allow_quoted_nulls = false).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { chooseFile, closeModal, expand, importDialog, menu, runJob, sqliteBlock } from "../transfer-helpers.mjs";

const NAME = "QA SQLite";

export function setup({ dir }) {
  new DatabaseSync(join(dir, "t.db")).close();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${sqliteBlock(NAME, join(dir, "t.db"))}`);
  writeFileSync(join(dir, "empty_vs_null.csv"), 'id,note\n1,""\n2,\n');
}

export default async function (page, { dir }) {
  await expand(page, NAME, "main");
  await menu(page, "main", "Import data into a new table", importDialog, NAME);
  assert.equal(await chooseFile(page, join(dir, "empty_vs_null.csv")), null);
  assert.match(await runJob(page, importDialog, "Import"), /Imported 2 rows/);
  await closeModal(page, importDialog);
  const db = new DatabaseSync(join(dir, "t.db"));
  const rows = db.prepare("SELECT id, note FROM empty_vs_null ORDER BY id").all().map((r) => ({ ...r }));
  db.close();
  assert.deepEqual(rows, [
    { id: 1, note: "" },
    { id: 2, note: null },
  ]);
}
