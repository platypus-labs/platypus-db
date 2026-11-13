// QA5-S17 (medium). The SQLite table editor creates a table again (a column's type changed) without
// the DEFERRABLE INITIALLY DEFERRED of its foreign keys: after Save, inserts that need the key
// checked at COMMIT (parent and child in one transaction, in either order) fail.
// Steps: t (id, r INT REFERENCES f0(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED, n INT); table editor:
// n to TEXT > Save.
// Expected: the new CREATE TABLE keeps DEFERRABLE INITIALLY DEFERRED on the key. Actual: ON DELETE CASCADE only.
// Code: crates/platypus-schema/src/lib.rs `load` (SQLite foreign keys read from pragma_foreign_key_list, which
// does not say deferrable) and ddl.rs (ForeignKeyDesign has no deferrable field).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { lab, saveEngines, seed } from "../schema-helpers.mjs";
import { needsEngines, SRC } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("sqlite");

export function setup({ dir }) {
  seed(dir, "sqlite", `CREATE TABLE f0 (id INTEGER PRIMARY KEY); CREATE TABLE t (id INTEGER PRIMARY KEY, r INT REFERENCES f0(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED, n INT);`);
  saveEngines(dir, ["sqlite"]);
}

export default async function (page, { dir }) {
  const l = await lab(page, dir, "sqlite");
  const r = await l.edit("t", (d) => {
    d.columns.find((c) => c.name === "n").data_type = "TEXT";
  });
  assert.equal(r.error, null, r.sql);
  const db = new DatabaseSync(join(dir, `${SRC}.db`));
  try {
    const sql = db.prepare("SELECT sql FROM sqlite_master WHERE name = 't'").get().sql;
    assert.match(sql, /DEFERRABLE\s+INITIALLY\s+DEFERRED/i, `the foreign key after the table was created again:\n${sql}\nscript:\n${r.sql}`);
  } finally {
    db.close();
  }
}
