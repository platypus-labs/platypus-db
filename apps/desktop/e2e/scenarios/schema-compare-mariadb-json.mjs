// SCH4-11 (high). Compare with... on MariaDB fails for every table with a JSON column: MariaDB's JSON is
// LONGTEXT plus an implicit CHECK (json_valid(col)) named after the column, the comparison copies that
// check as an explicit `CONSTRAINT j CHECK (json_valid(`j`))` next to `j json COLLATE utf8mb4_bin`, and the
// CREATE TABLE fails with "Duplicate CHECK constraint name 'j'".
// Steps: qa3s_src.d (id int PRIMARY KEY, j json); qa3s_dst empty; Compare with... > QA3 MariaDB -
// qa3s_dst; run the script.
// Expected: the script runs; a second comparison finds nothing. Actual: ERROR 1826 Duplicate CHECK
// constraint name 'j'.
// Code: crates/platypus-schema/src/lib.rs design_of (constraints: every CHECK the catalog lists, the
// implicit json_valid one included) and the MariaDB JSON column type; ddl.rs Ddl::constraint.
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines, told } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("mariadb");

export function setup({ dir }) {
  ENGINES.mariadb.seed(dir, SRC, `CREATE TABLE S.d (id int PRIMARY KEY, j json);`);
  ENGINES.mariadb.seed(dir, DST, "");
  saveEngines(dir, ["mariadb"]);
}

export default async function (page) {
  const p = PLACES.mariadb;
  try {
    await openPath(page, [...p.chain, p.last]);
    const r = await roundTrip(page, p.chain, p.last, p.target, p.targetSchema);
    assert.ok(r.sql, told(r));
    assert.equal(r.runError, null, `the script fails: ${r.runError}\n${r.sql}`);
    assert.equal(r.second.same, true, told(r));
  } finally {
    ENGINES.mariadb.drop(SRC);
    ENGINES.mariadb.drop(DST);
  }
}
