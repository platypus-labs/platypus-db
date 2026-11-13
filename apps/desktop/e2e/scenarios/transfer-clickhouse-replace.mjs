// XFER-11 (medium). "Replace all rows" fails on ClickHouse.
// Steps: a ClickHouse MergeTree table (id Int32, name String) with one row; a CSV `id,name /
// 2,new`; navigator › table › "Import data…", "Replace all rows", Import.
// Expected: the table holds only (2, new).
// Actual: "Nothing was imported: Syntax error: failed at position 26 (end of query): .
// Expected one of: ON, IN PARTITION, WHERE" — the import runs `DELETE FROM table`, which
// ClickHouse refuses without WHERE (TRUNCATE TABLE, or DELETE … WHERE 1, would do). The
// dialog also offers "Update rows with the same key", which then fails with "ClickHouse has
// no upsert".
// Suspected: crates/platypus-transfer/src/import.rs:546-548, and
// apps/desktop/src/transfer/ImportDataDialog.tsx:411-427 (modes offered whatever the database).
// Needs ClickHouse from docker/compose.yml.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { chooseFile, closeModal, db, dbQuiet, DBS, importDialog, menu, runJob, saveConnections, showTable, skipUnless } from "../transfer-helpers.mjs";

const TABLE = "qa_xfer_replace";

export const skip = () => skipUnless("clickhouse");

export function setup({ dir }) {
  saveConnections(dir, ["clickhouse"]);
  db("clickhouse", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id Int32, name String) ENGINE = MergeTree ORDER BY id; INSERT INTO ${TABLE} VALUES (1, 'old')`);
  writeFileSync(join(dir, "new.csv"), "id,name\n2,new\n");
}

export default async function (page, { dir }) {
  try {
    await showTable(page, DBS.clickhouse.name, "platypus", TABLE);
    await menu(page, TABLE, "Import data", importDialog, [DBS.clickhouse.name, "platypus"]);
    assert.equal(await chooseFile(page, join(dir, "new.csv")), null);
    await page.waitFor(`${importDialog}.querySelectorAll('.import-mapping select').length === 2`, 15_000);
    await page.eval(`[...${importDialog}.querySelectorAll('.import-modes label')].find(l => l.textContent.startsWith('Replace')).querySelector('input').click()`);
    const out = await runJob(page, importDialog, "Import");
    await closeModal(page, importDialog);
    assert.match(out, /Imported 1 row/);
    assert.equal(db("clickhouse", `SELECT groupArray(name) FROM ${TABLE}`), "['new']");
  } finally {
    dbQuiet("clickhouse", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
