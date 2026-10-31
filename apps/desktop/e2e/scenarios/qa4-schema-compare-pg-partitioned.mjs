// SCH4-10 (medium). Compare with... on PostgreSQL creates a partitioned table as a plain table: the
// parent loses PARTITION BY and its partitions are not created at all, with no warning, and a second
// comparison reports the schemas as the same.
// Steps: qa3s_src.ev (id int, ts date NOT NULL, v text, PRIMARY KEY (id, ts)) PARTITION BY RANGE (ts)
// with partitions ev_2024 and ev_2025; qa3s_dst empty; Compare with... > QA3 PG - qa3s_dst; run it.
// Expected: the target ev is partitioned (relkind p) with its partitions, or the script says it cannot
// copy partitions. Actual: "CREATE TABLE qa3s_dst.ev (...)" plain (relkind r), no partitions, no warning.
// Code: crates/platypus-schema/src/lib.rs load() (no partition clause read for PostgreSQL) and
// ddl.rs Ddl::create_table.
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, psql, roundTrip, saveEngines, told } from "../qa3-schema.mjs";

export const skip = needsEngines("pg");

export function setup({ dir }) {
  ENGINES.pg.seed(dir, SRC, `CREATE TABLE S.ev (id int, ts date NOT NULL, v text, PRIMARY KEY (id, ts)) PARTITION BY RANGE (ts);
CREATE TABLE S.ev_2024 PARTITION OF S.ev FOR VALUES FROM ('2024-01-01') TO ('2025-01-01');
CREATE TABLE S.ev_2025 PARTITION OF S.ev FOR VALUES FROM ('2025-01-01') TO ('2026-01-01');`);
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
    const kinds = psql(`SELECT string_agg(relname || ':' || relkind::text, ',' ORDER BY relname) FROM pg_class WHERE relnamespace = '${DST}'::regnamespace AND relkind IN ('r', 'p')`);
    assert.equal(kinds, "ev:p,ev_2024:r,ev_2025:r", `${kinds}\n${r.sql}`);
  } finally {
    ENGINES.pg.drop(SRC);
    ENGINES.pg.drop(DST);
  }
}

