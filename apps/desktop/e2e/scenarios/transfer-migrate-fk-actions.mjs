// XFER-18 (medium). "Copy schema to a database" drops the actions of foreign keys.
// Steps: a PostgreSQL schema with parent (id PRIMARY KEY) and child (parent_id REFERENCES
// parent ON DELETE CASCADE); navigator › schema › "Copy schema to a database…" › a SQLite
// connection, Copy.
// Expected: the copied foreign key keeps ON DELETE CASCADE (deleting a parent still deletes
// its children).
// Actual: the key is created without its action (NO ACTION). Seen into SQLite, MySQL and SQL
// Server (sys.foreign_keys.delete_referential_action_desc = NO_ACTION); ON UPDATE and SET NULL
// are lost the same way. The behaviour of the copied database differs, silently.
// Suspected: crates/platypus-transfer/src/migrate.rs:420-430 and 573-583 (the FOREIGN KEY
// clause has no ON DELETE / ON UPDATE), platypus_core::ForeignKeyMeta (no actions read).
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { closeModal, db, dbQuiet, DBS, expand, fill, menu, migrateDialog, row, runJob, saveConnections, skipUnless, sqliteBlock } from "../transfer-helpers.mjs";

const SCHEMA = "qa_xfer_fka";

export const skip = () => skipUnless("pg");

export function setup({ dir }) {
  new DatabaseSync(join(dir, "copy.db")).close();
  saveConnections(dir, ["pg"], sqliteBlock("QA SQLite", join(dir, "copy.db")));
  db(
    "pg",
    `DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE; CREATE SCHEMA ${SCHEMA};
     CREATE TABLE ${SCHEMA}.parent (id int PRIMARY KEY);
     CREATE TABLE ${SCHEMA}.child (id int PRIMARY KEY, parent_id int REFERENCES ${SCHEMA}.parent (id) ON DELETE CASCADE);
     INSERT INTO ${SCHEMA}.parent VALUES (1); INSERT INTO ${SCHEMA}.child VALUES (10, 1);`,
  );
}

export default async function (page, { dir }) {
  try {
    await expand(page, DBS.pg.name, null);
    await page.waitFor(`${row(SCHEMA, DBS.pg.name)} !== undefined`, 20_000);
    await menu(page, SCHEMA, "Copy schema to a database", migrateDialog, DBS.pg.name);
    const id = await page.eval(`[...${migrateDialog}.querySelector('select').options].find(o => o.textContent === 'QA SQLite').value`);
    if (await page.eval(`${migrateDialog}.querySelector('select').value !== ${JSON.stringify(id)}`)) await page.eval(fill(`${migrateDialog}.querySelector('select')`, id));
    await page.waitFor(`${migrateDialog}.querySelectorAll('select')[1]?.value === 'main'`, 20_000);
    assert.match(await runJob(page, migrateDialog, "Copy"), /Copied 2 rows/);
    await closeModal(page, migrateDialog);
    const copy = new DatabaseSync(join(dir, "copy.db"));
    const { sql } = copy.prepare("SELECT sql FROM sqlite_master WHERE name = 'child'").get();
    copy.close();
    // Deleting the parent then fails with "FOREIGN KEY constraint failed" instead of cascading.
    assert.match(sql, /ON DELETE CASCADE/i, sql);
  } finally {
    dbQuiet("pg", `DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE`);
  }
}
