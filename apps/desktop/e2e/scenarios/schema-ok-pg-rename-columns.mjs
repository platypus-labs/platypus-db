// Regression guard (passes): the PostgreSQL table editor when columns are renamed. A column in a UNIQUE
// constraint, a foreign key, a partial index, an expression index, a multi-column index and a CHECK is
// renamed the way the form does it (the other parts follow the new name): the script is the three
// RENAME COLUMN statements, runs, and every dependent object follows the column.
import assert from "node:assert/strict";
import { ENGINES, Q4, dropAll, lab, saveEngines, seed } from "../schema-helpers.mjs";
import { needsEngines } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("pg");

export function setup({ dir }) {
  seed(dir, "pg", `CREATE TABLE S.parent (id serial PRIMARY KEY);
CREATE TABLE S.child (id int PRIMARY KEY, parent_id int REFERENCES S.parent(id) ON DELETE CASCADE, v int CHECK (v > 0), name text NOT NULL, UNIQUE(parent_id, name));
CREATE INDEX child_name_idx ON S.child (lower(name));
CREATE INDEX child_v_idx ON S.child (v) WHERE v > 5;
CREATE INDEX child_multi ON S.child (name, v);`);
  saveEngines(dir, ["pg"]);
}

export default async function (page, { dir }) {
  try {
    const l = await lab(page, dir, "pg");
    const r = await l.edit("child", (d) => {
      for (const [from, to] of [["name", "title"], ["v", "val"], ["parent_id", "pid"]]) {
        d.columns.find((c) => c.name === from).name = to;
        for (const part of [...d.indexes, ...d.foreign_keys, ...d.constraints]) part.columns = part.columns.map((c) => (c === from ? to : c));
      }
    });
    assert.equal(r.error, null, r.sql);
    assert.equal((r.sql.match(/RENAME COLUMN/g) ?? []).length, 3, r.sql);
    const defs = ENGINES.pg.query(`SELECT indexdef FROM pg_indexes WHERE schemaname = '${Q4}' AND tablename = 'child' ORDER BY indexname`);
    assert.match(defs, /lower\(title\)/);
    assert.match(defs, /\(val\) WHERE \(val > 5\)/);
    assert.match(defs, /\(pid, title\)/);
    assert.equal(ENGINES.pg.query(`SELECT string_agg(pg_get_constraintdef(oid), '; ' ORDER BY conname) FROM pg_constraint WHERE conrelid = '${Q4}.child'::regclass AND contype IN ('f', 'c')`).trim(),
      "FOREIGN KEY (pid) REFERENCES qa3s_q4.parent(id) ON DELETE CASCADE; CHECK ((val > 0))".replace("qa3s_q4", Q4));
  } finally {
    dropAll("pg");
  }
}
