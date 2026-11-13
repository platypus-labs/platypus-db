// XFER-20 (medium). "Copy to a database" is offered for MongoDB, then fails with mongosh errors.
// Steps: a MongoDB collection with two documents; navigator › collection › "Copy to a
// database…" › target the PostgreSQL connection.
// Expected: the documents copied into a new PostgreSQL table (fields as columns, arrays and
// sub-documents as JSON), or the entry not offered for a collection.
// Actual: the preview fails at once: "unknown statement `SELECT`: expected `db.`, `show` or
// `use`" — the dialog reads the source with SQL. The other way round, MongoDB is listed as a
// target of a SQL table's copy and fails with "unknown statement `CREATE`".
// Suspected: apps/desktop/src/transfer/actions.ts:28-53 (when: isRelation, any driver),
// apps/desktop/src-tauri/src/commands/transfer.rs:130-152 (SQL preview on the source) and
// apps/desktop/src/transfer/ImportDataDialog.tsx:299-306 (MongoDB among the targets).
// Needs PostgreSQL and MongoDB from docker/compose.yml.
import assert from "node:assert/strict";
import { closeModal, copyTarget, db, dbQuiet, DBS, importDialog, menu, runJob, saveConnections, showTable, skipUnless } from "../transfer-helpers.mjs";

const COLLECTION = "qa_xfer_mcopy";

export const skip = () => skipUnless("pg", "mongo");

export function setup({ dir }) {
  saveConnections(dir, ["pg", "mongo"]);
  db("mongo", `db.${COLLECTION}.drop(); db.${COLLECTION}.insertMany([{ _id: 1, name: 'a', tags: ['x'] }, { _id: 2, name: 'b', tags: [] }]); 'ok'`);
  db("pg", `DROP TABLE IF EXISTS ${COLLECTION}`);
}

export default async function (page) {
  try {
    await showTable(page, DBS.mongo.name, "platypus", COLLECTION);
    await menu(page, COLLECTION, "Copy to a database", importDialog, [DBS.mongo.name, "platypus"]);
    assert.equal(await copyTarget(page, DBS.pg.name), null);
    assert.match(await runJob(page, importDialog, "Copy"), /Imported 2 rows/);
    await closeModal(page, importDialog);
    assert.equal(db("pg", `SELECT count(*) FROM ${COLLECTION}`), "2");
  } finally {
    dbQuiet("mongo", `db.${COLLECTION}.drop()`);
    dbQuiet("pg", `DROP TABLE IF EXISTS ${COLLECTION}`);
  }
}
