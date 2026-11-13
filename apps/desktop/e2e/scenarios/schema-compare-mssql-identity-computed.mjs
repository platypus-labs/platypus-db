// SCH4-04 (high). Compare with... on SQL Server creates IDENTITY columns as plain NOT NULL ints and
// computed columns as plain nullable columns (and drops COLLATE), and the second comparison reports
// the schemas as the same.
// Steps: qa3s_src.t (id int IDENTITY(10,5) PRIMARY KEY, a int NOT NULL, g AS (a * 2) PERSISTED,
// h AS (a + 1)); qa3s_dst empty; Compare with... QA3 MSSQL - qa3s_dst; run the script.
// Expected: the target keeps IDENTITY on id and the computed columns g and h (sys.columns
// is_identity / is_computed). Actual: all three flags are 0; INSERT INTO qa3s_dst.t (a) fails
// ("Cannot insert the value NULL into column 'id'").
// Code: crates/platypus-schema/src/lib.rs load(): no SQL Server branch for identity/computed/collation
// (the table editor's ALTER COLUMN on such a column has the same blind spot); ddl.rs Ddl::column.
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines, told } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("mssql");

export function setup({ dir }) {
  ENGINES.mssql.seed(dir, SRC, `SET QUOTED_IDENTIFIER ON;\nGO\nCREATE TABLE S.t (id int IDENTITY(10,5) PRIMARY KEY, a int NOT NULL, g AS (a * 2) PERSISTED, h AS (a + 1));`);
  ENGINES.mssql.seed(dir, DST, "");
  saveEngines(dir, ["mssql"]);
}

export default async function (page) {
  const p = PLACES.mssql;
  try {
    await openPath(page, [...p.chain, p.last]);
    const r = await roundTrip(page, p.chain, p.last, p.target, p.targetSchema);
    assert.ok(r.sql, told(r));
    assert.equal(r.runError, null, `the script fails: ${r.runError}\n${r.sql}`);
    const flags = ENGINES.mssql.query(
      `SELECT STRING_AGG(name + ':' + CAST(is_identity AS varchar) + CAST(is_computed AS varchar), ',') WITHIN GROUP (ORDER BY column_id) FROM sys.columns WHERE object_id = OBJECT_ID('${DST}.t')`,
    );
    assert.equal(flags.trim(), "id:10,a:00,g:01,h:01", `${flags}\n${r.sql}`);
  } finally {
    ENGINES.mssql.drop(SRC);
    ENGINES.mssql.drop(DST);
  }
}

