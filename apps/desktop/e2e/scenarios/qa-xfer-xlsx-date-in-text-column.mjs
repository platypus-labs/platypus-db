// XFER-15 (medium). A date cell of an Excel column that also holds text becomes a number.
// Steps: fixtures/qa-xfer-dates.xlsx, sheet "Data": column `mixed` holds the text "n/a" and,
// below it, the date 2024-01-01 (a date-formatted cell); navigator › main › "Import data into
// a new table…", Import.
// Expected: the second row's `mixed` is the date as text (2024-01-01, as the `when` column of
// dates shows it), the way Excel displays it.
// Actual: "45292", Excel's serial day number.
// Suspected: crates/platypus-driver-duckdb/src/files.rs:95-107 (cell_value() writes
// Data::DateTime with to_string() in a VARCHAR column).
import assert from "node:assert/strict";
import { copyFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { chooseFile, closeModal, expand, importDialog, menu, runJob, sqliteBlock } from "../qa-xfer.mjs";

const NAME = "QA SQLite";
const FIXTURE = join(dirname(fileURLToPath(import.meta.url)), "..", "fixtures", "qa-xfer-dates.xlsx");

export function setup({ dir }) {
  new DatabaseSync(join(dir, "t.db")).close();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${sqliteBlock(NAME, join(dir, "t.db"))}`);
  copyFileSync(FIXTURE, join(dir, "dates.xlsx"));
}

export default async function (page, { dir }) {
  await expand(page, NAME, "main");
  await menu(page, "main", "Import data into a new table", importDialog, NAME);
  assert.equal(await chooseFile(page, join(dir, "dates.xlsx")), null);
  assert.match(await runJob(page, importDialog, "Import"), /Imported 2 rows/);
  await closeModal(page, importDialog);
  const db = new DatabaseSync(join(dir, "t.db"));
  const rows = db.prepare('SELECT "when", mixed FROM "Data" ORDER BY id').all().map((r) => ({ ...r }));
  db.close();
  // The date column is right: the same date in a text column should read the same.
  assert.match(String(rows[0].when), /^2024-01-01/);
  assert.equal(rows[0].mixed, "n/a");
  assert.match(String(rows[1].mixed), /^2024-01-01/);
}
