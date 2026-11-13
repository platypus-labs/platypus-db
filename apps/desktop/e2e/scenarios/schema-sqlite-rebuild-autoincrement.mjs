// SCH4-06 (medium). A table editor save that makes SQLite rebuild the table (here: a NOT NULL column
// with a default is added) resets the AUTOINCREMENT counter: DROP TABLE removes the table's row of
// sqlite_sequence and the INSERT ... SELECT that copies the rows sets it to the highest id left. Ids of
// rows deleted before are handed out again, which AUTOINCREMENT exists to prevent (a deleted order's
// number is reused by a new order).
// Steps: t (id INTEGER PRIMARY KEY AUTOINCREMENT, v TEXT) with ids 1, 2, 3, then DELETE id 3 (seq = 3);
// table editor > add column z TEXT NOT NULL DEFAULT 'q' > Save; INSERT a row.
// Expected: the new row gets id 4 (sqlite_sequence is kept at 3). Actual: id 3.
// Code: crates/platypus-schema/src/ddl.rs Ddl::rebuild (no UPDATE sqlite_sequence after the copy).
import assert from "node:assert/strict";
import { DatabaseSync } from "node:sqlite";
import { join } from "node:path";
import { SRC, needsEngines } from "../schema-compare-helpers.mjs";
import { lab, saveEngines, seed } from "../schema-helpers.mjs";

export const skip = needsEngines("sqlite");

export function setup({ dir }) {
  seed(dir, "sqlite", `CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT, v TEXT);
INSERT INTO t (v) VALUES ('a'), ('b'), ('c');
DELETE FROM t WHERE id = 3;`);
  saveEngines(dir, ["sqlite"]);
}

export default async function (page, { dir }) {
  const l = await lab(page, dir, "sqlite");
  const r = await l.edit("t", (d) => {
    d.columns.push({ original: null, name: "z", data_type: "TEXT", nullable: false, default: "'q'", primary_key: false, comment: null });
  });
  assert.equal(r.error, null, r.sql);
  const db = new DatabaseSync(join(dir, `${SRC}.db`));
  try {
    db.exec("INSERT INTO t (v) VALUES ('new')");
    assert.equal(db.prepare("SELECT id FROM t WHERE v = 'new'").get().id, 4, `the counter went back:\n${r.sql}`);
  } finally {
    db.close();
  }
}
