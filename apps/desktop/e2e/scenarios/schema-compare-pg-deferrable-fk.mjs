// QA5-S6 (medium). Compare with... on PostgreSQL writes a DEFERRABLE foreign key as an
// immediate one: the script leaves out DEFERRABLE [INITIALLY DEFERRED], and a second comparison
// finds nothing to change. Loads and circular inserts that need the check at COMMIT fail in the target.
// Steps: b.a_id REFERENCES a(id) ON DELETE SET NULL DEFERRABLE INITIALLY DEFERRED; compare with an empty schema; run.
// Expected: pg_constraint.condeferrable and condeferred are true in the target. Actual: both false.
// Code: crates/platypus-schema/src/lib.rs `load` (only confdeltype/confupdtype are read) and
// crates/platypus-driver-pg/src/meta.rs foreign_keys; ForeignKeyDesign has no deferrable field.
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../schema-compare-helpers.mjs";

const KEY = "pg";
export const skip = needsEngines(KEY);
const SRC_SQL = `CREATE TABLE S.a (id int PRIMARY KEY); CREATE TABLE S.b (id int PRIMARY KEY, a_id int, CONSTRAINT b_a FOREIGN KEY (a_id) REFERENCES S.a(id) ON DELETE SET NULL DEFERRABLE INITIALLY DEFERRED);`;

export function setup({ dir }) {
  
  ENGINES[KEY].seed(dir, SRC, SRC_SQL);
  ENGINES[KEY].seed(dir, DST, "");
  saveEngines(dir, [KEY]);
}

const read = (sql) => String(ENGINES[KEY].query(sql)).replace(/\r/g, "").trim();

export default async function (page) {
  const p = PLACES[KEY];
  try {
    await openPath(page, [...p.chain, p.last]);
    const r = await roundTrip(page, p.chain, p.last, p.target, p.targetSchema);
    assert.ok(r.sql, `Compare gave no script: ${JSON.stringify(r.first)}`);
    assert.equal(r.runError, null, r.sql);
    assert.equal(read(`SELECT condeferrable || '/' || condeferred FROM pg_constraint WHERE conname = 'b_a' AND connamespace = '${DST}'::regnamespace`), "true/true");
  } finally {
    
    ENGINES[KEY].drop(SRC);
    ENGINES[KEY].drop(DST);
  }
}

