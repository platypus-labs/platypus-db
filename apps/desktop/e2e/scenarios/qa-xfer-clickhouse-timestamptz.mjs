// XFER-01 (critical). Timestamps with a time zone become NULL in ClickHouse.
// Steps: a PostgreSQL table with a `timestamptz` value; navigator › table › "Copy to a
// database…", target the ClickHouse connection, new table, Copy.
// Expected: the ClickHouse column holds 2024-06-01 08:00:00 (the same instant).
// Actual: the copy reports "Imported 1 row" but the column is NULL: the value is written as the
// literal '2024-06-01T08:00:00Z', which ClickHouse does not parse into DateTime64, and the
// Nullable column takes NULL silently. The same happens to "Copy schema to a database" and to
// a CSV/Parquet file with zoned timestamps imported into ClickHouse.
// Suspected: crates/platypus-sql/src/dml.rs:178-185 (the zoned timestamp literal is fixed for
// MySQL only).
// Needs PostgreSQL and ClickHouse from docker/compose.yml.
import assert from "node:assert/strict";
import { copyTarget, db, dbQuiet, DBS, importDialog, menu, runJob, closeModal, saveConnections, showTable, skipUnless } from "../qa-xfer.mjs";

const TABLE = "qa_xfer_tz";

export const skip = () => skipUnless("pg", "clickhouse");

export function setup({ dir }) {
  saveConnections(dir, ["pg", "clickhouse"]);
  db("pg", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, at timestamptz); INSERT INTO ${TABLE} VALUES (1, '2024-06-01 10:00:00+02')`);
  db("clickhouse", `DROP TABLE IF EXISTS ${TABLE}`);
}

export default async function (page) {
  try {
    await showTable(page, DBS.pg.name, "public", TABLE);
    await menu(page, TABLE, "Copy to a database", importDialog, [DBS.pg.name, "public"]);
    assert.equal(await copyTarget(page, DBS.clickhouse.name), null);
    const out = await runJob(page, importDialog, "Copy");
    assert.match(out, /Imported 1 row/);
    await closeModal(page, importDialog);
    assert.equal(db("clickhouse", `SELECT toString(at) FROM ${TABLE}`), "2024-06-01 08:00:00.000000");
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${TABLE}`);
    dbQuiet("clickhouse", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
