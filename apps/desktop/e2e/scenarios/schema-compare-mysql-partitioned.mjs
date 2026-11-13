// QA5-S10 (medium). Compare with... on MySQL creates a partitioned table as a plain one, with no
// warning (PostgreSQL's partitioned tables were fixed, see the PG scenario for partitions).
// Steps: pt PARTITION BY RANGE (YEAR(d)) with two partitions; compare with an empty schema; run.
// Expected: the target is partitioned (or the script says it cannot be). Actual: CREATE TABLE without PARTITION BY.
// Code: crates/platypus-schema (TableDesign.options is read for PostgreSQL and ClickHouse, not for MySQL SHOW CREATE TABLE).
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../schema-compare-helpers.mjs";

const KEY = "mysql";
export const skip = needsEngines(KEY);
const SRC_SQL = `CREATE TABLE S.pt (id int NOT NULL, d date NOT NULL, PRIMARY KEY (id, d)) PARTITION BY RANGE (YEAR(d)) (PARTITION p0 VALUES LESS THAN (2020), PARTITION p1 VALUES LESS THAN MAXVALUE);`;

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
    assert.equal(read(`SELECT count(partition_name) FROM information_schema.partitions WHERE table_schema = '${DST}' AND table_name = 'pt'`), "2");
  } finally {
    
    ENGINES[KEY].drop(SRC);
    ENGINES[KEY].drop(DST);
  }
}
