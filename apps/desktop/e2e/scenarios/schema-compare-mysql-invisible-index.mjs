// QA5-S12 (low). Compare with... on MySQL 8 makes an INVISIBLE index visible: the optimizer in the
// target starts using an index the DBA had hidden to test dropping it.
// Steps: b KEY i2 (b) INVISIBLE; compare with an empty schema; run.
// Expected: i2 is invisible in the target. Actual: information_schema.statistics.IS_VISIBLE = YES.
// Code: crates/platypus-schema/src/create.rs IndexDefinition (parsed from SHOW CREATE TABLE without the INVISIBLE word) / ddl.rs.
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../schema-compare-helpers.mjs";

const KEY = "mysql";
export const skip = needsEngines(KEY);
const SRC_SQL = `CREATE TABLE S.b (id int PRIMARY KEY, b int, KEY i2 (b) INVISIBLE);`;

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
    assert.equal(read(`SELECT is_visible FROM information_schema.statistics WHERE table_schema = '${DST}' AND index_name = 'i2'`), "NO");
  } finally {
    
    ENGINES[KEY].drop(SRC);
    ENGINES[KEY].drop(DST);
  }
}
