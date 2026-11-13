// SCH-11 (medium). On SQL Server two identical schemas are not "the same" for Compare with… when
// a table has an unnamed UNIQUE constraint: SQL Server names it UQ__customer__<random>, the
// names differ between the schemas, and the script drops the target's constraint and adds it
// again under the source's generated name. The same happens on Oracle (SYS_C…, see SCH-05).
// Steps: qa3s_src and qa3s_dst created by the same script (customers.email varchar(100) NOT
// NULL UNIQUE); navigator > qa3s_src > Compare with… > qa3s_dst > Compare.
// Expected: "nothing to change". Actual: DROP CONSTRAINT UQ__customer__AB6E61641177B78C;
// ADD CONSTRAINT UQ__customer__AB6E6164B991975D UNIQUE (email).
// Suspected: crates/platypus-schema/src/compare.rs:124-130 (constraints matched by name; only
// SQLite's unnamed ones are matched by kind and columns; system-named ones should be too).
import assert from "node:assert/strict";
import { BASE, DST, ENGINES, PLACES, SRC, compareUi, needsEngines, openPath, saveEngines } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("mssql");

export function setup({ dir }) {
  ENGINES.mssql.seed(dir, SRC, BASE.mssql);
  ENGINES.mssql.seed(dir, DST, BASE.mssql);
  saveEngines(dir, ["mssql"]);
}

export default async function (page) {
  const { chain, last, target, targetSchema } = PLACES.mssql;
  const problems = [];
  try {
    await openPath(page, [...chain, last]);
    const same = await compareUi(page, chain, last, target, targetSchema);
    if (same.same !== true) problems.push(`identical schemas get a script:\n${same.sql ?? same.error}`);
  } finally {
    ENGINES.mssql.drop(SRC);
    ENGINES.mssql.drop(DST);
  }
  assert.deepEqual(problems, []);
}
