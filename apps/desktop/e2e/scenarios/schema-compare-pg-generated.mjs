// SCH4-01 (high). Compare with... on PostgreSQL turns a STORED generated column into a plain column
// with the expression as its DEFAULT, so the script it opens fails.
// Steps: qa3s_src.t (id int PRIMARY KEY, a int NOT NULL, g int GENERATED ALWAYS AS (a * 2) STORED);
// qa3s_dst empty; navigator > qa3s_src > Compare with... > QA3 PG - qa3s_dst; run the script.
// Expected: the script creates g as GENERATED ALWAYS AS (a * 2) STORED, runs, and a second comparison
// finds nothing. Actual: "g integer DEFAULT (a * 2)" in the script; running it fails with
// "cannot use column reference in DEFAULT expression".
// Code: crates/platypus-schema/src/lib.rs load(): only MySQL (mysql_columns), SQLite and DuckDB fill
// ColumnDesign.generated; PostgreSQL's generated columns come from table_details as a default,
// and ddl.rs Ddl::column writes `DEFAULT (expr)`.
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, psql, roundTrip, saveEngines, told } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("pg");

export function setup({ dir }) {
  ENGINES.pg.seed(dir, SRC, `CREATE TABLE S.t (id int PRIMARY KEY, a int NOT NULL, g int GENERATED ALWAYS AS (a * 2) STORED);`);
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
    assert.match(
      psql(`SELECT coalesce(generation_expression, 'none') FROM information_schema.columns WHERE table_schema = '${DST}' AND table_name = 't' AND column_name = 'g'`),
      /a \* 2/,
    );
    assert.equal(r.second.same, true, told(r));
  } finally {
    ENGINES.pg.drop(SRC);
    ENGINES.pg.drop(DST);
  }
}

