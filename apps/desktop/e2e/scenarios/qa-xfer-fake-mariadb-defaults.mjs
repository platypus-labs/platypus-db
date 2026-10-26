// XFER-08 (medium). On MariaDB fake data leaves every column but the key empty.
// Steps: a MariaDB table (id int PRIMARY KEY, name varchar(50), qty int) without defaults;
// navigator › table › "Generate data…".
// Expected: name suggested "Full name" and qty "Whole number", as on MySQL.
// Actual: both are suggested "Default (not filled)": MariaDB's information_schema gives
// COLUMN_DEFAULT = 'NULL' (the text) for a column without a default, the app reads it as a
// default, and the generated rows hold only keys and NULLs.
// Suspected: the MySQL driver's column metadata (COLUMN_DEFAULT 'NULL' on MariaDB, see
// crates/platypus-driver-mysql) and apps/desktop/src/transfer/generators.ts:30.
// Needs MariaDB from docker/compose.yml.
import assert from "node:assert/strict";
import { db, dbQuiet, DBS, generateDialog, menu, closeModal, saveConnections, showTable, skipUnless } from "../qa-xfer.mjs";

const TABLE = "qa_xfer_fk_maria";

export const skip = () => skipUnless("mariadb");

export function setup({ dir }) {
  saveConnections(dir, ["mariadb"]);
  db("mariadb", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, name varchar(50), qty int)`);
}

export default async function (page) {
  try {
    await showTable(page, DBS.mariadb.name, "platypus", TABLE);
    await menu(page, TABLE, "Generate data", generateDialog, [DBS.mariadb.name, "platypus"]);
    await page.waitFor(`${generateDialog}.querySelectorAll('select').length === 3`, 20_000);
    const kinds = await page.eval(`[...${generateDialog}.querySelectorAll('select')].map(s => s.value)`);
    await closeModal(page, generateDialog);
    assert.deepEqual(kinds, ["next_key", "full_name", "integer"]);
  } finally {
    dbQuiet("mariadb", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
