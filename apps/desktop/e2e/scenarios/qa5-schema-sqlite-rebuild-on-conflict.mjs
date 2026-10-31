// QA5-S18 (low). The SQLite table editor creates a table again without the conflict clause of a UNIQUE
// column (UNIQUE ON CONFLICT REPLACE, used for "last one wins" tables): afterwards a duplicate raises
// an error instead of replacing the row.
// Steps: t (id, u TEXT UNIQUE ON CONFLICT REPLACE, n INT); table editor: n to TEXT > Save.
// Expected: the constraint keeps ON CONFLICT REPLACE. Actual: UNIQUE (u).
// Code: crates/platypus-schema/src/ddl.rs / create.rs (constraints are rebuilt from columns; the clause is not parsed).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { lab, saveEngines, seed } from "../qa4-schema.mjs";
import { needsEngines, SRC } from "../qa3-schema.mjs";

export const skip = needsEngines("sqlite");

export function setup({ dir }) {
  seed(dir, "sqlite", `CREATE TABLE t (id INTEGER PRIMARY KEY, u TEXT UNIQUE ON CONFLICT REPLACE, n INT);`);
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
    assert.match(sql, /ON\s+CONFLICT\s+REPLACE/i, `the unique constraint after the table was created again:\n${sql}\nscript:\n${r.sql}`);
  } finally {
    db.close();
  }
}
