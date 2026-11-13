// SCH4-09 (high). Compare with... on PostgreSQL writes the type of an enum (or domain) column
// qualified with the SOURCE schema: `m qa3s_src.mood`. Against another database the script fails
// (the type is not there); against another schema of the same database it runs and the target table
// then depends on the source schema's type (dropping qa3s_src fails or needs CASCADE, which drops the
// target's column). The type itself is not created in the target either.
// Steps: qa3s_src.mood ENUM ('sad','ok'); qa3s_src.e (id int PRIMARY KEY, m qa3s_src.mood NOT NULL
// DEFAULT 'ok'); qa3s_dst empty; Compare with... > QA3 PG - qa3s_dst; run the script; then
// DROP SCHEMA qa3s_src (no CASCADE).
// Expected: the target column has its own type in qa3s_dst (or the script creates it), so the source
// schema can be dropped. Actual: "m qa3s_src.mood NOT NULL DEFAULT 'ok'::qa3s_src.mood"; the DROP fails
// with "cannot drop schema qa3s_src because other objects depend on it".
// Code: crates/platypus-schema/src/compare.rs (matched() rewrites the schema of foreign keys only) and
// the PostgreSQL design read (user-defined types come schema-qualified); same family as qa3 SCH-04.
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, psql, roundTrip, saveEngines, told } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("pg");

export function setup({ dir }) {
  ENGINES.pg.seed(dir, SRC, `CREATE TYPE S.mood AS ENUM ('sad', 'ok');
CREATE TABLE S.e (id int PRIMARY KEY, m S.mood NOT NULL DEFAULT 'ok');`);
  ENGINES.pg.seed(dir, DST, "");
  saveEngines(dir, ["pg"]);
}

export default async function (page) {
  const p = PLACES.pg;
  try {
    await openPath(page, [...p.chain, p.last]);
    const r = await roundTrip(page, p.chain, p.last, p.target, p.targetSchema);
    assert.ok(r.sql, told(r));
    assert.equal(r.runError, null, `the script fails: ${r.runError}\n${r.sql}`);
    const statements = r.sql.replace(/^--.*$/gm, "");
    assert.doesNotMatch(statements, /qa3s_src/, `the script names the source schema:\n${r.sql}`);
    // Dropping the source must leave the target alone.
    psql(`DROP SCHEMA ${SRC} CASCADE`);
    assert.equal(psql(`SELECT count(*) FROM information_schema.columns WHERE table_schema = '${DST}' AND table_name = 'e'`), "2");
  } finally {
    ENGINES.pg.drop(SRC);
    ENGINES.pg.drop(DST);
  }
}
