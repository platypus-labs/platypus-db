// QA5-S16 (medium). Compare with... on MariaDB creates a system-versioned table (WITH SYSTEM
// VERSIONING) as an ordinary one, with no warning: the row history the source keeps is not kept
// in the target, and a second comparison reports nothing to change.
// Steps: sv (id, v) WITH SYSTEM VERSIONING; compare with an empty schema; run.
// Expected: the target table is system-versioned (or the script says it cannot be).
// Actual: information_schema.tables.table_type = BASE TABLE (SYSTEM VERSIONED in the source).
// Code: crates/platypus-schema/src/create.rs parse (the clause after the column list is kept for other
// dialects only), crates/platypus-schema/src/lib.rs `load`.
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../qa3-schema.mjs";

export const skip = needsEngines("mariadb");
const e = ENGINES.mariadb;

export function setup({ dir }) {
  e.seed(dir, SRC, "CREATE TABLE S.sv (id int PRIMARY KEY, v int) WITH SYSTEM VERSIONING;");
  e.seed(dir, DST, "");
  saveEngines(dir, ["mariadb"]);
}

export default async function (page) {
  const p = PLACES.mariadb;
  try {
    await openPath(page, [...p.chain, p.last]);
    const r = await roundTrip(page, p.chain, p.last, p.target, p.targetSchema);
    assert.ok(r.sql, `Compare gave no script: ${JSON.stringify(r.first)}`);
    assert.equal(r.runError, null, r.sql);
    const kind = e.root(`SELECT table_type FROM information_schema.tables WHERE table_schema = '${DST}' AND table_name = 'sv'`).trim();
    assert.equal(kind, "SYSTEM VERSIONED", r.sql);
  } finally {
    e.drop(SRC);
    e.drop(DST);
  }
}
