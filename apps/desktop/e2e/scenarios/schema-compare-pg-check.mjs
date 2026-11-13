// SCH-03 (medium). On PostgreSQL a comparison never settles once it has copied a CHECK with
// IN (…): the copied check reads back from pg_get_constraintdef in another form
// (ARRAY['new'::character varying::text, …] instead of ARRAY['new'::character varying, …]::text[]),
// so every later Compare with… drops and re-adds it, and "nothing to change" never shows.
// Steps: qa3s_src.t has CHECK (status IN ('new','paid')), qa3s_dst.t has none; Compare with…
// qa3s_dst, run the script, compare again.
// Expected: the second comparison finds nothing to change.
// Actual: it proposes DROP CONSTRAINT t_status_check / ADD CONSTRAINT t_status_check again.
// Suspected: the constraint definitions compared as text in crates/platypus-schema/src/ddl.rs
// (script, via compare.rs matched); the definition should be normalised (or compared through
// the database, e.g. by creating it and reading it back) before deciding it changed.
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, needsEngines, openPath, roundTrip, saveEngines, told } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("pg");

export function setup({ dir }) {
  ENGINES.pg.seed(dir, SRC, `CREATE TABLE S.t (id int PRIMARY KEY, status varchar(20) CHECK (status IN ('new','paid')), qty int CHECK (qty > 0));`);
  ENGINES.pg.seed(dir, DST, `CREATE TABLE S.t (id int PRIMARY KEY, status varchar(20), qty int);`);
  saveEngines(dir, ["pg"]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA3 PG", "platypus", SRC]);
    const r = await roundTrip(page, ["QA3 PG", "platypus"], SRC, "QA3 PG", DST);
    assert.ok(r.sql, told(r));
    assert.equal(r.runError, null, `the script fails:\n${told(r)}`);
    assert.equal(r.second.same, true, `a second comparison still finds changes:\n${told(r)}`);
  } finally {
    ENGINES.pg.drop(SRC);
    ENGINES.pg.drop(DST);
  }
}
