// XFER-14 (high). Decimals of a CSV lose digits on import, whatever the target type.
// Steps: a CSV `id,amount / 1,12345678901234567.123456789`; navigator › main › "Import data
// into a new table…"; the type of amount is changed from REAL to TEXT so nothing is lost on
// the SQLite side; Import.
// Expected: amount holds 12345678901234567.123456789.
// Actual: DuckDB infers DOUBLE for the column and the import writes the double's text
// (1.2345678901234568e16): the table holds '12345678901234568.0' — 10 of the 27 digits are
// gone, without a warning. On PostgreSQL the same file lands as 1.2345678901234568e+16 in the
// suggested DOUBLE PRECISION column, and NUMERIC(30,9) gets the rounded literal too.
// Suspected: crates/platypus-driver-duckdb/src/files.rs:56-60 (read_csv infers DOUBLE for
// decimals; no DECIMAL detection or all-text read) and
// crates/platypus-transfer/src/import.rs:437-454 (values written from the source type).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { chooseFile, closeModal, expand, fill, importDialog, menu, runJob, sqliteBlock } from "../transfer-helpers.mjs";

const NAME = "QA SQLite";

export function setup({ dir }) {
  new DatabaseSync(join(dir, "t.db")).close();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${sqliteBlock(NAME, join(dir, "t.db"))}`);
  writeFileSync(join(dir, "money.csv"), "id,amount\n1,12345678901234567.123456789\n");
}

export default async function (page, { dir }) {
  await expand(page, NAME, "main");
  await menu(page, "main", "Import data into a new table", importDialog, NAME);
  assert.equal(await chooseFile(page, join(dir, "money.csv")), null);
  const type = `${importDialog}.querySelector('input[aria-label="Type of amount"]')`;
  await page.eval(fill(type, "TEXT"));
  await page.waitFor(`${type}.value === 'TEXT'`);
  assert.match(await runJob(page, importDialog, "Import"), /Imported 1 row/);
  await closeModal(page, importDialog);
  const db = new DatabaseSync(join(dir, "t.db"));
  const { amount } = db.prepare("SELECT amount FROM money").get();
  db.close();
  assert.equal(amount, "12345678901234567.123456789");
}
