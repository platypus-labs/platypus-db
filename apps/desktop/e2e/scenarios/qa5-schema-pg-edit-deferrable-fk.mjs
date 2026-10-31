// QA5-S19 (medium). The PostgreSQL table editor drops DEFERRABLE when a foreign key is edited: the key
// is dropped and added again without it (the design has no field for it), so after changing ON DELETE the
// key is checked at each statement.
// Steps: b.a_id REFERENCES a(id) ON DELETE SET NULL DEFERRABLE INITIALLY DEFERRED; table editor on b:
// ON DELETE to CASCADE > Save.
// Expected: the new key is still DEFERRABLE INITIALLY DEFERRED. Actual: condeferrable = false.
// Code: crates/platypus-schema/src/lib.rs `load` and ddl.rs (ForeignKeyDesign has no deferrable field).
import assert from "node:assert/strict";
import { ENGINES, Q4, dropAll, lab, saveEngines, seed } from "../qa4-schema.mjs";
import { needsEngines } from "../qa3-schema.mjs";

export const skip = needsEngines("pg");

export function setup({ dir }) {
  seed(dir, "pg", `CREATE TABLE S.a (id int PRIMARY KEY); CREATE TABLE S.b (id int PRIMARY KEY, a_id int, CONSTRAINT b_a FOREIGN KEY (a_id) REFERENCES S.a(id) ON DELETE SET NULL DEFERRABLE INITIALLY DEFERRED);`);
  saveEngines(dir, ["pg"]);
}

export default async function (page, { dir }) {
  try {
    const l = await lab(page, dir, "pg");
    const r = await l.edit("b", (d) => {
      d.foreign_keys[0].on_delete = "CASCADE";
    });
    assert.equal(r.error, null, r.sql);
    const row = ENGINES.pg.query(`SELECT confdeltype::text || '/' || condeferrable || '/' || condeferred FROM pg_constraint WHERE conname = 'b_a' AND connamespace = '${Q4}'::regnamespace`).trim();
    assert.equal(row, "c/true/true", r.sql);
  } finally {
    dropAll("pg");
  }
}

