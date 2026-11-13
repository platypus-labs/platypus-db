// qa4-xfer-4 (high). A failing CSV import into a ClickHouse table says "Nothing was imported" but
// leaves most of the rows in the table.
// Steps: ClickHouse table (id Int32, n Int32, s String) empty; a CSV of 20,000 good rows and a last
// row `20001,abc,x`; navigator › table › "Import data…", choose the file, Import.
// Expected: the message tells the truth. Either the table is empty (the load went through a staging
// table / was undone) or the message says how many rows were already written and are still there.
// Actual: "Nothing was imported: Cannot parse string 'abc' as Int32 ..." while the table holds
// about 19,900 of the rows (PostgreSQL, MySQL and SQL Server really roll back and hold 0).
// Retrying the import after fixing the file then duplicates those rows.
// Code: the ClickHouse import path in crates/platypus-transfer/src/import.rs (batched INSERTs,
// no undo) and its error text.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { chooseFile, closeModal, db, dbQuiet, DBS, importDialog, menu, runJob, saveConnections, showTable, skipUnless } from "../transfer-helpers.mjs";

const TABLE = "qa4x_partial";

export const skip = () => skipUnless("clickhouse");

export function setup({ dir }) {
  saveConnections(dir, ["clickhouse"]);
  db("clickhouse", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id Int32, n Int32, s String) ENGINE=MergeTree ORDER BY id`);
  let csv = "id,n,s\n";
  for (let i = 1; i <= 20000; i++) csv += `${i},${i},v${i}\n`;
  writeFileSync(join(dir, "bad.csv"), csv + "20001,abc,x\n");
}

export default async function (page, { dir }) {
  try {
    await showTable(page, DBS.clickhouse.name, "platypus", TABLE);
    await menu(page, TABLE, "Import data", importDialog, [DBS.clickhouse.name, "platypus"]);
    assert.equal(await chooseFile(page, join(dir, "bad.csv")), null);
    const out = await runJob(page, importDialog, "Import", 120_000);
    await closeModal(page, importDialog);
    const left = Number(db("clickhouse", `SELECT count() FROM ${TABLE}`));
    if (/Nothing was imported/.test(out)) assert.equal(left, 0, `${out.slice(0, 80)} but ${left} rows are in the table`);
  } finally {
    dbQuiet("clickhouse", `DROP TABLE ${TABLE}`);
  }
}
