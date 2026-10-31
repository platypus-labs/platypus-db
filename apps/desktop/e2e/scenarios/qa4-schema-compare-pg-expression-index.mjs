// SCH4-03 (medium). Compare with... on PostgreSQL writes an expression index's expression as a quoted
// column name, so the script fails; the index opts (USING gin/brin, DESC, WHERE) are not carried either.
// Steps: qa3s_src.t (id int PRIMARY KEY, name text, ts timestamptz) with
// CREATE INDEX t_lower ON t (lower(name)), CREATE UNIQUE INDEX t_part ON t (name) WHERE id > 5 and
// CREATE INDEX t_brin ON t USING brin (ts); qa3s_dst empty; Compare with... > run the script.
// Expected: the script runs and each index is created as in the source (expression, WHERE, brin).
// Actual: `CREATE INDEX t_lower ON qa3s_dst.t ("lower(name)")` fails: column "lower(name)" does not exist;
// (without that index) t_part is created UNIQUE without its WHERE and t_brin as btree.
// Code: crates/platypus-schema/src/lib.rs design_of (IndexDesign.sql is filled for SQLite only) and
// ddl.rs Ddl::create_index (quotes every column).
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, psql, roundTrip, saveEngines, told } from "../qa3-schema.mjs";

export const skip = needsEngines("pg");

export function setup({ dir }) {
  ENGINES.pg.seed(dir, SRC, `CREATE TABLE S.t (id int PRIMARY KEY, name text, ts timestamptz);
CREATE INDEX t_lower ON S.t (lower(name));
CREATE UNIQUE INDEX t_part ON S.t (name) WHERE id > 5;
CREATE INDEX t_brin ON S.t USING brin (ts);`);
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
    const defs = psql(`SELECT indexdef FROM pg_indexes WHERE schemaname = '${DST}' AND tablename = 't' AND indexname LIKE 't\\_%' ORDER BY indexname`);
    assert.match(defs, /t_brin .*USING brin/s, defs);
    assert.match(defs, /t_part .*WHERE/s, defs);
    assert.match(defs, /t_lower .*lower\(name\)/s, defs);
  } finally {
    ENGINES.pg.drop(SRC);
    ENGINES.pg.drop(DST);
  }
}
