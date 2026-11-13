// XFER-27 (low). A PostgreSQL CHECK with a cast is copied verbatim to MySQL and left out.
// Steps: a PostgreSQL schema with a table (id int PRIMARY KEY, total numeric(10,2) CHECK
// (total >= 0)); navigator › schema › "Copy schema to a database…" › MySQL, Copy.
// Expected: the CHECK added on MySQL as total >= 0 (MySQL 8 enforces CHECKs).
// Actual: "Left out: …_total_check: 1064: You have an error in your SQL syntax … near
// '::numeric)'": PostgreSQL stores the constraint as `(total >= (0)::numeric)` and the copy
// sends that text to MySQL as is. SQL Server fails the same way ("Incorrect syntax near
// '::'"). The constraint is lost, with a raw syntax error as the explanation.
// Suspected: crates/platypus-transfer/src/migrate.rs (CHECK definitions are not translated
// between dialects; at least `::type` casts could be dropped).
// Needs PostgreSQL and MySQL from docker/compose.yml.
import assert from "node:assert/strict";
import { closeModal, db, dbQuiet, DBS, expand, fill, menu, migrateDialog, row, runJob, saveConnections, skipUnless } from "../transfer-helpers.mjs";

const SCHEMA = "qa_xfer_ck";
const TABLE = "qa_xfer_ck_t";

export const skip = () => skipUnless("pg", "mysql");

export function setup({ dir }) {
  saveConnections(dir, ["pg", "mysql"]);
  db("pg", `DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE; CREATE SCHEMA ${SCHEMA}; CREATE TABLE ${SCHEMA}.${TABLE} (id int PRIMARY KEY, total numeric(10,2) CHECK (total >= 0)); INSERT INTO ${SCHEMA}.${TABLE} VALUES (1, 5)`);
  db("mysql", `DROP TABLE IF EXISTS ${TABLE}`);
}

export default async function (page) {
  try {
    await expand(page, DBS.pg.name, null);
    await page.waitFor(`${row(SCHEMA, DBS.pg.name)} !== undefined`, 20_000);
    await menu(page, SCHEMA, "Copy schema to a database", migrateDialog, DBS.pg.name);
    const id = await page.eval(`[...${migrateDialog}.querySelector('select').options].find(o => o.textContent === ${JSON.stringify(DBS.mysql.name)}).value`);
    if (await page.eval(`${migrateDialog}.querySelector('select').value !== ${JSON.stringify(id)}`)) await page.eval(fill(`${migrateDialog}.querySelector('select')`, id));
    await page.waitFor(`${migrateDialog}.querySelectorAll('select')[1]?.value === 'platypus'`, 20_000);
    const out = await runJob(page, migrateDialog, "Copy");
    await closeModal(page, migrateDialog);
    assert.match(out, /Copied 1 rows/);
    assert.doesNotMatch(out, /check/i, out);
  } finally {
    dbQuiet("pg", `DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE`);
    dbQuiet("mysql", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
