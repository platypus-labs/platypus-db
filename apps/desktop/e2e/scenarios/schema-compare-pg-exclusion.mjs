// QA5-S9 (medium). Compare with... on PostgreSQL leaves an EXCLUDE constraint out of the script,
// silently, and a second comparison reports nothing to change: a reservation table copied this way
// no longer refuses overlapping ranges.
// Steps: ex (id, r int4range, EXCLUDE USING gist (r WITH &&)); compare with an empty schema; run.
// Expected: the target has the exclusion constraint (contype x). Actual: the table has none.
// Code: crates/platypus-schema/src/lib.rs design_of (ConstraintKind::Exclusion is read by the driver,
// ConstraintDesign only knows UNIQUE and CHECK).
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../schema-compare-helpers.mjs";

const KEY = "pg";
export const skip = needsEngines(KEY);
const SRC_SQL = `CREATE TABLE S.ex (id int, r int4range, CONSTRAINT ex_r EXCLUDE USING gist (r WITH &&));`;

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
    assert.equal(read(`SELECT count(*) FROM pg_constraint WHERE contype = 'x' AND connamespace = '${DST}'::regnamespace`), "1");
  } finally {
    
    ENGINES[KEY].drop(SRC);
    ENGINES[KEY].drop(DST);
  }
}
