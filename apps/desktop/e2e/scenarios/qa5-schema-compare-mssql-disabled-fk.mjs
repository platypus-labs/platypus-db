// QA5-S14 (medium). Compare with... on SQL Server enables a foreign key or check constraint that the
// source has disabled (ALTER TABLE ... NOCHECK CONSTRAINT): the script creates it enabled and
// trusted, so the rows that the source holds (and that break the rule) cannot be loaded.
// Steps: b.fk_b disabled with NOCHECK; compare with an empty schema; run.
// Expected: fk_b is_disabled = 1 in the target. Actual: 0.
// Code: crates/platypus-schema (ForeignKeyDesign / ConstraintDesign have no disabled flag; the driver does not read sys.foreign_keys.is_disabled).
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../qa3-schema.mjs";

const KEY = "mssql";
export const skip = needsEngines(KEY);
const SRC_SQL = `CREATE TABLE S.a (id int PRIMARY KEY); CREATE TABLE S.b (id int PRIMARY KEY, a_id int, CONSTRAINT fk_b FOREIGN KEY (a_id) REFERENCES S.a(id)); ALTER TABLE S.b NOCHECK CONSTRAINT fk_b;`;

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
    assert.equal(read(`SELECT is_disabled FROM sys.foreign_keys WHERE name = 'fk_b' AND schema_id = SCHEMA_ID('${DST}')`), "1");
  } finally {
    
    ENGINES[KEY].drop(SRC);
    ENGINES[KEY].drop(DST);
  }
}
