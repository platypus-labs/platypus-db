// SCH-02 (high). On MySQL 8 a CHECK constraint with a string literal is copied by Compare
// with… (and by the table editor's DDL) with the backslash escapes of
// information_schema.CHECK_CONSTRAINTS: CHECK ((`status` in (_utf8mb4\'new\',_utf8mb4\'paid\')))
// which MySQL rejects ("You have an error in your SQL syntax"). MariaDB is fine.
// Steps: database qa3s_src with t(status CHECK (status IN ('new','paid'))), qa3s_dst empty;
// navigator > qa3s_src > Compare with… > qa3s_dst > Compare; run the script; compare again.
// Expected: the script runs and the second comparison finds nothing to change.
// Actual: CREATE TABLE fails with a syntax error near '\'new\''.
// Suspected: crates/platypus-driver-mysql/src/meta.rs:351 (CHECK_CLAUSE used as it is; MySQL
// escapes quotes in it).
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, needsEngines, openPath, roundTrip, saveEngines, told } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("mysql");

export function setup({ dir }) {
  ENGINES.mysql.seed(dir, SRC, `CREATE TABLE t (id int PRIMARY KEY, status varchar(10), note varchar(20) DEFAULT 'it''s',
  CONSTRAINT t_status CHECK (status IN ('new','paid')));`);
  ENGINES.mysql.seed(dir, DST, "");
  saveEngines(dir, ["mysql"]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA3 MySQL", SRC]);
    const r = await roundTrip(page, ["QA3 MySQL"], SRC, "QA3 MySQL", DST);
    assert.ok(r.sql, told(r));
    assert.doesNotMatch(r.sql, /\\'/, `the script keeps MySQL's escapes:\n${r.sql}`);
    assert.equal(r.runError, null, `the script fails:\n${told(r)}`);
    assert.equal(r.second.same, true, `a second comparison still finds changes:\n${told(r)}`);
  } finally {
    ENGINES.mysql.drop(SRC);
    ENGINES.mysql.drop(DST);
  }
}
