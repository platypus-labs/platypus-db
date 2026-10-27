// DBS-19 (low): on MariaDB, `DELETE HISTORY FROM t` (which erases every past version of a
// system-versioned table) is confirmed as "DELETE without WHERE changes every row of HISTORY":
// the confirmation names a table HISTORY, not t, and says it changes current rows, which it
// does not touch.
//
// Steps: MariaDB (port 53307); CREATE TABLE qa_dbs_sv (id INT PRIMARY KEY, v INT) WITH SYSTEM
// VERSIONING, one row updated once; editor, run DELETE HISTORY FROM qa_dbs_sv.
// Expected: the confirmation names qa_dbs_sv (and says it deletes its history).
// Actual: "DELETE without WHERE changes every row of HISTORY (row count unknown)".
// Suspected: crates/platypus-sql/src/analysis.rs `unfiltered` / MODIFIERS (HISTORY is not
// skipped as a modifier, so it is read as the table name).
import assert from "node:assert/strict";
import { conns, needsPorts, sh } from "../qa-dbs.mjs";
import { openEditor, runOrGuard } from "../qa-safe-db.mjs";

export const skip = needsPorts(53307);

export function setup({ dir }) {
  sh("mariadb", "DROP TABLE IF EXISTS qa_dbs_sv; CREATE TABLE qa_dbs_sv (id INT PRIMARY KEY, v INT) WITH SYSTEM VERSIONING; INSERT INTO qa_dbs_sv VALUES (1, 1); UPDATE qa_dbs_sv SET v = 2;");
  conns(dir, [["QA Maria", "mariadb"]]);
}

export default async function (page) {
  try {
    await openEditor(page, "QA Maria");
    const r = await runOrGuard(page, "DELETE HISTORY FROM qa_dbs_sv");
    assert.match(r, /^guard: /, r);
    assert.match(r, /qa_dbs_sv/, `the confirmation names the table: ${r}`);
    assert.equal(sh("mariadb", "SELECT count(*) FROM qa_dbs_sv FOR SYSTEM_TIME ALL"), "2");
  } finally {
    sh("mariadb", "DROP TABLE IF EXISTS qa_dbs_sv");
  }
}
