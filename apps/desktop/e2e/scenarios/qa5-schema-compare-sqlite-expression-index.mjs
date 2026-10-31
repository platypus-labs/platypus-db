// QA5-S2 (high). Compare with... on SQLite: an index on an expression is written with the
// placeholder the driver reports for it, "(expression)", as if it were a column. The script
// runs (SQLite reads the double-quoted name as a string) and creates an index on a constant,
// so the target has an index that does nothing and the real one is missing.
// Steps: e1 (id, name TEXT, n INT) with CREATE INDEX e1_e ON e1 (lower(name), n DESC); compare
// with an empty schema; run the script.
// Expected: e1_e in the target is on lower(name), n DESC. Actual: CREATE INDEX main.e1_e ON e1 ("(expression)", n).
// Code: crates/platypus-schema/src/compare.rs / diff.rs (index columns taken from IndexMeta.columns,
// where the SQLite driver puts "(expression)"); the design keeps the index's own sql for the editor, compare does not.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../qa3-schema.mjs";

export const skip = needsEngines("sqlite");
const SRC_SQL = `CREATE TABLE e1 (id INTEGER PRIMARY KEY, name TEXT, n INT); CREATE INDEX e1_e ON e1 (lower(name), n DESC);`;
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
  const sql = sqlOf(dir, "e1_e");
  assert.match(sql, /lower\s*\(\s*name\s*\)/i, `the index in the target: ${sql}\nscript:\n${r.sql}`);
}
