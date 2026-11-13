// SCH-01 (high). Compare with… creates the new tables before it alters the existing ones, so a
// new table whose foreign key references a column (or a unique key) the same script adds to an
// existing table fails: "column "code" referenced in foreign key constraint does not exist".
// Same on MySQL, MariaDB, SQL Server and Oracle (SQLite does not check it at CREATE).
// Steps: qa3s_src has accounts(id, code UNIQUE) and vouchers(account_code → accounts.code);
// qa3s_dst has accounts(id) only. Navigator > qa3s_src > Compare with… > qa3s_dst > Compare,
// run the script, compare again.
// Expected: the script runs (ALTER accounts first, or the new tables' keys added at the end)
// and the second comparison finds nothing to change. Actual: CREATE TABLE vouchers fails.
// Suspected: crates/platypus-schema/src/compare.rs:30-56 (new tables scripted before the
// tables matched by name are altered).
import assert from "node:assert/strict";
import { DST, ENGINES, NEW_REF, SRC, needsEngines, openPath, roundTrip, saveEngines, told } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("pg");

export function setup({ dir }) {
  ENGINES.pg.seed(dir, SRC, NEW_REF.pg[0]);
  ENGINES.pg.seed(dir, DST, NEW_REF.pg[1]);
  saveEngines(dir, ["pg"]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA3 PG", "platypus", SRC]);
    const r = await roundTrip(page, ["QA3 PG", "platypus"], SRC, "QA3 PG", DST);
    assert.ok(r.sql, told(r));
    assert.match(r.sql, /CREATE TABLE qa3s_dst\.vouchers/);
    assert.equal(r.runError, null, `the script fails:\n${told(r)}`);
    assert.equal(r.second.same, true, `a second comparison still finds changes:\n${told(r)}`);
  } finally {
    ENGINES.pg.drop(SRC);
    ENGINES.pg.drop(DST);
  }
}
