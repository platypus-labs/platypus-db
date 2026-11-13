// QA5-S7 (medium). Compare with... on PostgreSQL 15+ writes UNIQUE NULLS NOT DISTINCT as a plain
// UNIQUE: in the target many rows with a NULL key are accepted that the source refuses.
// Steps: c.UNIQUE NULLS NOT DISTINCT (a1, a2); compare with an empty schema; run.
// Expected: the target constraint is UNIQUE NULLS NOT DISTINCT (a1, a2). Actual: UNIQUE (a1, a2).
// Code: crates/platypus-driver-pg/src/meta.rs constraints() / crates/platypus-schema ConstraintDesign
// (UNIQUE is rebuilt from its columns; the options of pg_get_constraintdef are not kept).
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../schema-compare-helpers.mjs";

const KEY = "pg";
export const skip = needsEngines(KEY);
const SRC_SQL = `CREATE TABLE S.c (id int PRIMARY KEY, a1 int, a2 int, CONSTRAINT c_u UNIQUE NULLS NOT DISTINCT (a1, a2));`;

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
    assert.match(read(`SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conname = 'c_u' AND connamespace = '${DST}'::regnamespace`), /NULLS NOT DISTINCT/);
  } finally {
    
    ENGINES[KEY].drop(SRC);
    ENGINES[KEY].drop(DST);
  }
}
