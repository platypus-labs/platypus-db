// QA5-S5 (medium). Compare with... on SQLite writes a foreign key without its
// DEFERRABLE INITIALLY DEFERRED, so in the target the check happens at each statement and the
// source's bulk loads / circular inserts inside a transaction fail.
// Steps: f1.r REFERENCES f0(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED; compare with an
// empty schema; run the script.
// Expected: the table in the target has DEFERRABLE INITIALLY DEFERRED. Actual: only ON DELETE CASCADE.
// Code: crates/platypus-schema/src/lib.rs load / catalog.rs (only on_delete/on_update are read: ForeignKeyDesign has no deferrable).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../qa3-schema.mjs";

export const skip = needsEngines("sqlite");
const SRC_SQL = `CREATE TABLE f0 (id INTEGER PRIMARY KEY); CREATE TABLE f1 (id INTEGER PRIMARY KEY, r INT REFERENCES f0(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED);`;
const DST_SQL = ``;

export function setup({ dir }) {
  ENGINES.sqlite.seed(dir, SRC, SRC_SQL);
  ENGINES.sqlite.seed(dir, DST, DST_SQL);
  saveEngines(dir, ["sqlite"]);
}

/** What sqlite_master of the target holds for `name`. */
const sqlOf = (dir, name) => {
  const db = new DatabaseSync(join(dir, `${DST}.db`));
  try {
    return db.prepare("SELECT sql FROM sqlite_master WHERE name = ?").get(name)?.sql ?? null;
  } finally {
    db.close();
  }
};

export default async function (page, { dir }) {
  const p = PLACES.sqlite;
  await openPath(page, [...p.chain, p.last]);
  const r = await roundTrip(page, p.chain, p.last, p.target, p.targetSchema);
  assert.ok(r.sql, `Compare gave no script: ${JSON.stringify(r.first)}`);
  assert.equal(r.runError, null);
  const sql = sqlOf(dir, "f1");
  assert.match(sql, /DEFERRABLE\s+INITIALLY\s+DEFERRED/i, `the table in the target: ${sql}`);
}
