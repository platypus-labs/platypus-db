// qa4-xfer-2 (medium). Copying a PostgreSQL table with a `time` column to MySQL turns
// 23:59:59.999999 into 24:00:00 (and drops every fraction).
// Steps: PostgreSQL table (id int, tm time) with tm = '23:59:59.999999' and '10:20:30.123456';
// navigator › table › "Copy to a database…" › QA MySQL (new table), Copy.
// Expected: the copy keeps the microseconds (the timestamp columns of the same copy become
// DATETIME(6)); the column type is TIME(6) and the values are equal to the source.
// Actual: the mapped type is TIME (no fractional seconds): 23:59:59.999999 is rounded UP to
// 24:00:00 and 10:20:30.123456 to 10:20:30.
// Code: the type mapping of Time to MySQL in crates/platypus-transfer (migrate.rs / import.rs):
// Timestamp maps to DATETIME(6), Time to plain TIME. The CSV import of a time column does the same.
import assert from "node:assert/strict";
import { closeModal, copyTarget, db, dbQuiet, DBS, importDialog, menu, runJob, saveConnections, showTable, skipUnless } from "../qa-xfer.mjs";

const TABLE = "qa4x_time";

export const skip = () => skipUnless("pg", "mysql");

export function setup({ dir }) {
  saveConnections(dir, ["pg", "mysql"]);
  db("pg", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, tm time); INSERT INTO ${TABLE} VALUES (1, '23:59:59.999999'), (2, '10:20:30.123456')`);
  dbQuiet("mysql", `DROP TABLE ${TABLE}`);
}

export default async function (page) {
  try {
    await showTable(page, DBS.pg.name, "public", TABLE);
    await menu(page, TABLE, "Copy to a database", importDialog, [DBS.pg.name, "public"]);
    assert.equal(await copyTarget(page, DBS.mysql.name), null);
    assert.match(await runJob(page, importDialog, "Copy"), /Imported 2 rows/);
    await closeModal(page, importDialog);
    assert.equal(db("mysql", `SELECT GROUP_CONCAT(CAST(tm AS CHAR) ORDER BY id) FROM ${TABLE}`), "23:59:59.999999,10:20:30.123456");
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${TABLE}`);
    dbQuiet("mysql", `DROP TABLE ${TABLE}`);
  }
}
