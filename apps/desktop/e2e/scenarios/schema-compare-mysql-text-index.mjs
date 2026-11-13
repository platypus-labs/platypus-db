// SCH4-12 (high). Compare with... on MySQL drops the prefix length of an index and the FULLTEXT kind: an
// index on a TEXT column (KEY d_body (body(50))) is created as `CREATE INDEX d_body ON d (body)`, which MySQL
// refuses ("BLOB/TEXT column 'body' used in key specification without a key length"), so the script
// fails after the table was created (MySQL commits each statement). A FULLTEXT index also becomes a
// plain one (and the one on a long VARCHAR hits "Specified key was too long").
// Steps: qa3s_src.d (id int PRIMARY KEY, body text, KEY d_body (body(50)), FULLTEXT KEY d_ft (body));
// qa3s_dst empty; Compare with... > QA3 MySQL - qa3s_dst; run the script.
// Expected: the script runs and the target has the prefix index and the FULLTEXT index
// (information_schema.STATISTICS SUB_PART / INDEX_TYPE). Actual: ERROR 1170 on d_body.
// Code: crates/platypus-schema/src/lib.rs design_of (IndexDesign has columns only, `sql` is read for
// SQLite alone) and ddl.rs Ddl::create_index.
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines, told } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("mysql");

export function setup({ dir }) {
  ENGINES.mysql.seed(dir, SRC, `CREATE TABLE S.d (id int PRIMARY KEY, body text, KEY d_body (body(50)), FULLTEXT KEY d_ft (body));`);
  ENGINES.mysql.seed(dir, DST, "");
  saveEngines(dir, ["mysql"]);
}

export default async function (page) {
  const p = PLACES.mysql;
  try {
    await openPath(page, [...p.chain, p.last]);
    const r = await roundTrip(page, p.chain, p.last, p.target, p.targetSchema);
    assert.ok(r.sql, told(r));
    assert.equal(r.runError, null, `the script fails: ${r.runError}\n${r.sql}`);
    const got = ENGINES.mysql.root(
      `SELECT GROUP_CONCAT(INDEX_NAME, ':', INDEX_TYPE, ':', IFNULL(SUB_PART, '-') ORDER BY INDEX_NAME) FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = '${DST}' AND TABLE_NAME = 'd' AND INDEX_NAME <> 'PRIMARY'`,
    ).trim();
    assert.equal(got, "d_body:BTREE:50,d_ft:FULLTEXT:-");
  } finally {
    ENGINES.mysql.drop(SRC);
    ENGINES.mysql.drop(DST);
  }
}
