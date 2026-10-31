// qa4-xfer-5 (high). Importing an empty CSV field into a non-Nullable ClickHouse column stores 0
// (or '') instead of failing, so a NULL silently becomes a value.
// Steps: ClickHouse table (id Int32, n Int32, d Date) ; CSV `id,n,d / 1,5,2024-01-02 / 2,,`;
// navigator › table › "Import data…", Import.
// Expected: the import fails on row 2 ("Nothing was imported", the column does not accept NULL), as
// PostgreSQL and SQL Server do for a NOT NULL column; at least it must not report success with
// invented values.
// Actual: "Imported 2 rows"; row 2 holds n = 0 and d = 1970-01-01 (ClickHouse's null-as-default).
// Code: the ClickHouse import (crates/platypus-transfer/src/import.rs) sends NULL through an insert
// that has input_format_null_as_default on (the driver's session settings).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { chooseFile, closeModal, db, dbQuiet, DBS, importDialog, menu, runJob, saveConnections, showTable, skipUnless } from "../qa-xfer.mjs";

const TABLE = "qa4x_nulls";

export const skip = () => skipUnless("clickhouse");

export function setup({ dir }) {
  saveConnections(dir, ["clickhouse"]);
  db("clickhouse", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id Int32, n Int32, d Date) ENGINE=MergeTree ORDER BY id`);
  writeFileSync(join(dir, "nulls.csv"), "id,n,d\n1,5,2024-01-02\n2,,\n");
}

export default async function (page, { dir }) {
  try {
    await showTable(page, DBS.clickhouse.name, "platypus", TABLE);
    await menu(page, TABLE, "Import data", importDialog, [DBS.clickhouse.name, "platypus"]);
    assert.equal(await chooseFile(page, join(dir, "nulls.csv")), null);
    const out = await runJob(page, importDialog, "Import");
    await closeModal(page, importDialog);
    assert.doesNotMatch(out, /^Imported/, `reported success: ${out}; table: ${JSON.stringify(db("clickhouse", `SELECT id, n, d FROM ${TABLE} ORDER BY id FORMAT CSV`))}`);
    assert.equal(db("clickhouse", `SELECT count() FROM ${TABLE}`), "0");
  } finally {
    dbQuiet("clickhouse", `DROP TABLE ${TABLE}`);
  }
}
