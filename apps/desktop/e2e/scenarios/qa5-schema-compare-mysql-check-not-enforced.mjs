// QA5-S11 (medium). Compare with... on MySQL 8 turns a NOT ENFORCED check into an enforced one,
// so the data the source holds (which breaks the rule) cannot be copied in.
// Steps: c.w with CONSTRAINT w_chk CHECK (w < 100) NOT ENFORCED; compare with an empty schema; run.
// Expected: w_chk is NOT ENFORCED in the target. Actual: ENFORCED = YES.
// Code: crates/platypus-schema (ConstraintDesign has no enforced flag); the driver reads information_schema.check_constraints only.
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../qa3-schema.mjs";

const KEY = "mysql";
export const skip = needsEngines(KEY);
const SRC_SQL = `CREATE TABLE S.c (id int PRIMARY KEY, w int, CONSTRAINT w_chk CHECK (w < 100) NOT ENFORCED);`;

export function setup({ dir }) {
  
  ENGINES[KEY].seed(dir, SRC, SRC_SQL);
  ENGINES[KEY].seed(dir, DST, "");
  saveEngines(dir, [KEY]);
}

const read = (sql) => String(ENGINES[KEY].root(sql)).replace(/\r/g, "").trim();

export default async function (page) {
  const p = PLACES[KEY];
  try {
    await openPath(page, [...p.chain, p.last]);
    const r = await roundTrip(page, p.chain, p.last, p.target, p.targetSchema);
    assert.ok(r.sql, `Compare gave no script: ${JSON.stringify(r.first)}`);
    assert.equal(r.runError, null, r.sql);
    assert.equal(read(`SELECT enforced FROM information_schema.table_constraints WHERE table_schema = '${DST}' AND constraint_name = 'w_chk'`), "NO");
  } finally {
    
    ENGINES[KEY].drop(SRC);
    ENGINES[KEY].drop(DST);
  }
}
