// QA5-S8 (low). Compare with... on PostgreSQL writes a MATCH FULL foreign key as MATCH SIMPLE: a
// composite key with one NULL column is accepted in the target and refused by the source.
// Steps: c (x, y) REFERENCES p(a, b) MATCH FULL; compare with an empty schema; run.
// Expected: the target key is MATCH FULL. Actual: pg_constraint.confmatchtype = 's'.
// Code: crates/platypus-schema/src/lib.rs `load` (only the delete and update actions are read).
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../qa3-schema.mjs";

const KEY = "pg";
export const skip = needsEngines(KEY);
const SRC_SQL = `CREATE TABLE S.p (a int, b int, PRIMARY KEY (a, b)); CREATE TABLE S.c (id int PRIMARY KEY, x int, y int, CONSTRAINT c_fk FOREIGN KEY (x, y) REFERENCES S.p(a, b) MATCH FULL);`;

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
    assert.equal(read(`SELECT confmatchtype FROM pg_constraint WHERE conname = 'c_fk' AND connamespace = '${DST}'::regnamespace`), "f");
  } finally {
    
    ENGINES[KEY].drop(SRC);
    ENGINES[KEY].drop(DST);
  }
}
