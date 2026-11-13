// qa4-xfer-3 (high). Copying a PostgreSQL table with old or far dates to ClickHouse silently
// changes the dates.
// Steps: PostgreSQL table (id, d date, ts timestamp) with 1000-01-01, 0001-01-01 and 9999-12-31;
// navigator › table › "Copy to a database…" › QA ClickHouse (new table), Copy.
// Expected: either the values arrive unchanged (e.g. the column is created as String) or the copy
// stops with an error saying the value is out of range, as MySQL does for NaN ("Data truncated").
// Actual: "Imported 3 rows"; ClickHouse's Date32 / DateTime64 clamp silently: 1000-01-01 and
// 0001-01-01 become 1900-01-01, 9999-12-31 becomes 2299-12-31. The data is wrong with no warning.
// Code: ClickHouse type mapping of Date/Timestamp in crates/platypus-transfer (migrate.rs) and the
// ClickHouse insert path, which do not check the Date32/DateTime64 range (1900-01-01..2299-12-31).
import assert from "node:assert/strict";
import { closeModal, copyTarget, db, dbQuiet, DBS, importDialog, menu, runJob, saveConnections, showTable, skipUnless } from "../transfer-helpers.mjs";

const TABLE = "qa4x_dates";

export const skip = () => skipUnless("pg", "clickhouse");

export function setup({ dir }) {
  saveConnections(dir, ["pg", "clickhouse"]);
  db("pg", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, d date, ts timestamp); INSERT INTO ${TABLE} VALUES (1, '1000-01-01', '1000-01-01 00:00:00'), (2, '0001-01-01', '0001-01-01 12:00:00'), (3, '9999-12-31', '9999-12-31 23:59:59')`);
  dbQuiet("clickhouse", `DROP TABLE ${TABLE}`);
}

export default async function (page) {
  try {
    await showTable(page, DBS.pg.name, "public", TABLE);
    await menu(page, TABLE, "Copy to a database", importDialog, [DBS.pg.name, "public"]);
    assert.equal(await copyTarget(page, DBS.clickhouse.name), null);
    const out = await runJob(page, importDialog, "Copy");
    await closeModal(page, importDialog);
    if (/Imported 3 rows/.test(out)) {
      // Imported means every value is there as it was.
      assert.equal(
        db("clickhouse", `SELECT arrayStringConcat(groupArray(toString(d)), ',') FROM (SELECT d FROM ${TABLE} ORDER BY id)`),
        "1000-01-01,0001-01-01,9999-12-31",
      );
    } else {
      assert.match(out, /out of range|too (big|large)|Nothing was imported/);
    }
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${TABLE}`);
    dbQuiet("clickhouse", `DROP TABLE ${TABLE}`);
  }
}
