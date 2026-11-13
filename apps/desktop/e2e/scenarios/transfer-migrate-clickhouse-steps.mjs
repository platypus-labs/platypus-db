// XFER-19 (medium). "Copy schema to a database" into ClickHouse fails every step after the rows.
// Steps: a PostgreSQL schema with a table (id int PRIMARY KEY, flag boolean DEFAULT false,
// name text) and an index on name; navigator › schema › "Copy schema to a database…" ›
// ClickHouse, Copy.
// Expected: the rows copied, the default applied (ClickHouse has column defaults), and what
// ClickHouse cannot have (foreign keys, secondary indexes) listed in plain words.
// Actual: "Copied 1 rows. Left out:" then, for the default and the index alike, "62: Syntax
// error: failed at position 6 (end of query): . Expected TRANSACTION. (SYNTAX_ERROR)": each
// step is wrapped in `BEGIN`, which ClickHouse rejects, so nothing after the rows is ever
// tried.
// Suspected: crates/platypus-transfer/src/migrate.rs:585-599 (apply() sends BEGIN whatever
// the target's capabilities).
// Needs PostgreSQL and ClickHouse from docker/compose.yml.
import assert from "node:assert/strict";
import { closeModal, db, dbQuiet, DBS, expand, fill, menu, migrateDialog, row, runJob, saveConnections, skipUnless } from "../transfer-helpers.mjs";

const SCHEMA = "qa_xfer_chm";
const TABLE = "qa_xfer_chm_t";

export const skip = () => skipUnless("pg", "clickhouse");

export function setup({ dir }) {
  saveConnections(dir, ["pg", "clickhouse"]);
  db(
    "pg",
    `DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE; CREATE SCHEMA ${SCHEMA};
     CREATE TABLE ${SCHEMA}.${TABLE} (id int PRIMARY KEY, flag boolean DEFAULT false, name text);
     CREATE INDEX ${TABLE}_name ON ${SCHEMA}.${TABLE} (name);
     INSERT INTO ${SCHEMA}.${TABLE} (id, name) VALUES (1, 'a');`,
  );
  db("clickhouse", `DROP TABLE IF EXISTS ${TABLE}`);
}

export default async function (page) {
  try {
    await expand(page, DBS.pg.name, null);
    await page.waitFor(`${row(SCHEMA, DBS.pg.name)} !== undefined`, 20_000);
    await menu(page, SCHEMA, "Copy schema to a database", migrateDialog, DBS.pg.name);
    const id = await page.eval(`[...${migrateDialog}.querySelector('select').options].find(o => o.textContent === ${JSON.stringify(DBS.clickhouse.name)}).value`);
    if (await page.eval(`${migrateDialog}.querySelector('select').value !== ${JSON.stringify(id)}`)) await page.eval(fill(`${migrateDialog}.querySelector('select')`, id));
    await page.waitFor(`${migrateDialog}.querySelectorAll('select')[1]?.value === 'platypus'`, 20_000);
    const out = await runJob(page, migrateDialog, "Copy");
    await closeModal(page, migrateDialog);
    assert.match(out, /Copied 1 rows/);
    assert.doesNotMatch(out, /Expected TRANSACTION|SYNTAX_ERROR/);
    assert.match(db("clickhouse", `SELECT default_expression FROM system.columns WHERE database = 'platypus' AND table = '${TABLE}' AND name = 'flag'`), /false|0/);
  } finally {
    dbQuiet("pg", `DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE`);
    dbQuiet("clickhouse", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
