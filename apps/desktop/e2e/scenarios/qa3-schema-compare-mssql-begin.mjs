// SCH-12 (high). On SQL Server the script Compare with… opens in the editor starts with
// "BEGIN;" and ends with "COMMIT;": BEGIN alone is not a statement in T-SQL, so running the
// script fails at its first line ("Incorrect syntax near ';'") and nothing is migrated. (The
// backend's own apply wraps the statements itself and works; the text shown does not.) Oracle
// gets the same BEGIN; (an empty PL/SQL block there).
// Steps: qa3s_src with a self-reference, composite keys, a cycle, odd names and a view;
// qa3s_dst a changed copy; navigator > qa3s_src > Compare with… > qa3s_dst > Compare; run the
// script (Script button); compare again.
// Expected: BEGIN TRANSACTION; … COMMIT; (or no wrapper), the script runs and the second
// comparison finds nothing to change. Actual: 102: Incorrect syntax near ';'.
// Suspected: crates/platypus-schema/src/lib.rs:141-157 (Script::text writes "BEGIN" for every
// dialect).
import assert from "node:assert/strict";
import { BASE, DST, ENGINES, PLACES, SRC, VARIANT, needsEngines, openPath, roundTrip, saveEngines, told } from "../qa3-schema.mjs";

export const skip = needsEngines("mssql");

export function setup({ dir }) {
  ENGINES.mssql.seed(dir, SRC, BASE.mssql);
  ENGINES.mssql.seed(dir, DST, VARIANT.mssql);
  saveEngines(dir, ["mssql"]);
}

export default async function (page) {
  const { chain, last, target, targetSchema } = PLACES.mssql;
  try {
    await openPath(page, [...chain, last]);
    const r = await roundTrip(page, chain, last, target, targetSchema);
    assert.ok(r.sql, told(r));
    assert.equal(r.runError, null, `the script fails:\n${told(r)}`);
    assert.doesNotMatch(r.sql, /^BEGIN;$/m, "a T-SQL script starting with BEGIN;");
    assert.equal(r.second.same, true, `a second comparison still finds changes:\n${told(r)}`);
  } finally {
    ENGINES.mssql.drop(SRC);
    ENGINES.mssql.drop(DST);
  }
}
