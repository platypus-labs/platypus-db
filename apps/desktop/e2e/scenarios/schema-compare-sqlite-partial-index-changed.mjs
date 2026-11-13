// QA5-S4 (medium). Compare with... on SQLite does not see a changed WHERE of a partial index:
// the columns are the same, so the schemas are "the same".
// Steps: q1 with CREATE INDEX q1_i ON q1 (n) WHERE n > 5 in the source and WHERE n > 9 in the
// target; compare.
// Expected: a script that recreates q1_i. Actual: "nothing to change".
// Code: crates/platypus-schema/src/compare.rs (indexes compared by name, columns and uniqueness only).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("sqlite");
const SRC_SQL = `CREATE TABLE q1 (id INTEGER PRIMARY KEY, n INT); CREATE INDEX q1_i ON q1 (n) WHERE n > 5;`;
const DST_SQL = `CREATE TABLE q1 (id INTEGER PRIMARY KEY, n INT); CREATE INDEX q1_i ON q1 (n) WHERE n > 9;`;

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
  assert.match(sqlOf(dir, "q1_i"), /n > 5/);
}
