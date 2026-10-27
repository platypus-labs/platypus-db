// DBS-10 (medium): MariaDB JSON columns are not recognised: MariaDB stores JSON as LONGTEXT
// with a CHECK (json_valid(col)), the app shows the column as "longtext", and "Generate
// data…" fills it with words, so the CHECK rejects the whole run ("CONSTRAINT `t.j` failed").
// The grid also edits it as plain text (no JSON editor), unlike MySQL's JSON.
//
// Steps: MariaDB (port 53307); qa_dbs_j (id INT PRIMARY KEY, j JSON); navigator › qa_dbs_j ›
// Generate data…, keep the suggestions, 10 rows.
// Expected: j recognised as JSON ("Document" generator), "Added 10 rows.".
// Actual: j gets "Words" (type longtext), "Nothing was added: CONSTRAINT `qa_dbs_j.j` failed
// for `platypus`.`qa_dbs_j`".
// Suspected: the MySQL driver's column metadata (crates/platypus-driver-mysql/src/meta.rs /
// columns.rs) does not read MariaDB's json_valid check (information_schema.CHECK_CONSTRAINTS)
// to report the type as json; generators.ts then picks "text".
import assert from "node:assert/strict";
import { conns, needsPorts, openPath, sh } from "../qa-dbs.mjs";
import { generate } from "../qa-xfer.mjs";

export const skip = needsPorts(53307);

export function setup({ dir }) {
  sh("mariadb", "DROP TABLE IF EXISTS qa_dbs_j; CREATE TABLE qa_dbs_j (id INT PRIMARY KEY, j JSON);");
  conns(dir, [["QA Maria", "mariadb"]]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA Maria", "platypus", "Tables", "qa_dbs_j"]);
    const { kinds, out } = await generate(page, "qa_dbs_j", ["QA Maria", "platypus"], 10);
    assert.match(out, /Added 10 rows/, `${out} (generators ${kinds})`);
    assert.equal(sh("mariadb", "SELECT count(*) FROM qa_dbs_j WHERE json_valid(j)"), "10");
  } finally {
    sh("mariadb", "DROP TABLE IF EXISTS qa_dbs_j");
  }
}
