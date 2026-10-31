// QA5-S3 (high). Compare with... on SQLite leaves the WHERE of a partial index out: a partial
// UNIQUE index (the usual "unique among the live rows") becomes a plain unique index in the
// target and rejects rows the source accepts. And two schemas whose partial indexes differ
// only in their WHERE are "the same".
// Steps: p1 (id, n INT, live INT) with CREATE UNIQUE INDEX p1_u ON p1 (n) WHERE live = 1; compare
// with an empty schema, run the script, insert (1, 5, 0), (2, 5, 0) in the target.
// Expected: p1_u keeps WHERE live = 1 and the two rows go in. Actual: CREATE UNIQUE INDEX main.p1_u ON p1 (n):
// the second insert fails with UNIQUE constraint failed.
// Code: crates/platypus-schema/src/compare.rs / diff.rs (IndexMeta has only `partial: bool`, no predicate).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../qa3-schema.mjs";

export const skip = needsEngines("sqlite");
const SRC_SQL = `CREATE TABLE p1 (id INTEGER PRIMARY KEY, n INT, live INT); CREATE UNIQUE INDEX p1_u ON p1 (n) WHERE live = 1;`;
const DST_SQL = ``;

export function setup({ dir }) {
  ENGINES.sqlite.seed(dir, SRC, SRC_SQL);
  ENGINES.sqlite.seed(dir, DST, DST_SQL);
  saveEngines(dir, ["sqlite"]);
}

/** What sqlite_master of the target holds for `name`. */
const sqlOf = (dir, name) => {
  const db = new DatabaseSync(join(dir, `${DST}.db`));
  try {
    return db.prepare("SELECT sql FROM sqlite_master WHERE name = ?").get(name)?.sql ?? null;
  } finally {
    db.close();
  }
};

export default async function (page, { dir }) {
  const p = PLACES.sqlite;
  await openPath(page, [...p.chain, p.last]);
  const r = await roundTrip(page, p.chain, p.last, p.target, p.targetSchema);
  assert.ok(r.sql, `Compare gave no script: ${JSON.stringify(r.first)}`);
  assert.equal(r.runError, null);
  const sql = sqlOf(dir, "p1_u");
  assert.match(sql, /WHERE\s+live\s*=\s*1/i, `the index in the target: ${sql}\nscript:\n${r.sql}`);
}
