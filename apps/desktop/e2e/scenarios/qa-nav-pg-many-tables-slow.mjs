// Bug NAV: on PostgreSQL, expanding a schema with 2000 tables takes about 6 s (and a Refresh as
// much again); the time grows with the square of the catalog's size. The tree itself renders
// fast (a click re-renders the 2000 rows in ~60 ms): the time is the objects query.
//
// Cause: `objects` in crates/platypus-driver-pg/src/meta.rs computes each table's size with a
// correlated subquery `SELECT sum(relpages) FROM pg_class r WHERE r.oid = c.oid OR r.oid =
// c.reltoastrelid OR r.oid IN (SELECT indexrelid …)`: the OR defeats the index on pg_class.oid,
// so pg_class is scanned once per table (5.1 s of the 5.2 s query here; 4 ms without it).
//
// Steps: a schema with 2000 tables; expand it in the navigator.
// Expected: the tables listed within 2 s. Actual: ~6 s.
import assert from "node:assert/strict";
import { DBS, connectTo, needs, row, saveConnections } from "../qa-nav-helpers.mjs";

export const skip = needs("postgres");

const SCHEMA = "qa_nav_many";

/** Drops the schema 400 tables at a time: one transaction for all would run out of locks. */
function dropSchema() {
  for (let from = 1; from <= 2000; from += 400) {
    DBS.postgres.exec(`DO $$ BEGIN FOR i IN ${from}..${from + 399} LOOP
      EXECUTE format('DROP TABLE IF EXISTS ${SCHEMA}.qa_nav_t%s', lpad(i::text, 4, '0'));
    END LOOP; END $$;`);
  }
  DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE;`);
}

export function setup({ dir }) {
  DBS.postgres.exec(`CREATE SCHEMA IF NOT EXISTS ${SCHEMA};`);
  dropSchema();
  DBS.postgres.exec(`CREATE SCHEMA ${SCHEMA};`);
  for (let from = 1; from <= 2000; from += 400) {
    DBS.postgres.exec(`DO $$ BEGIN FOR i IN ${from}..${from + 399} LOOP
      EXECUTE format('CREATE TABLE ${SCHEMA}.qa_nav_t%s (id int PRIMARY KEY, v text)', lpad(i::text, 4, '0'));
    END LOOP; END $$;`);
  }
  saveConnections(dir, DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a0015"));
}

export default async function (page) {
  try {
    await connectTo(page, "QA PG", "platypus");
    await page.waitFor(`${row(SCHEMA)} !== undefined`, 20_000);
    const started = Date.now();
    await page.eval(`${row(SCHEMA)}.querySelector('.twisty').click()`);
    await page.waitFor(`${row("qa_nav_t2000")} !== undefined`, 60_000);
    const ms = Date.now() - started;
    assert.ok(ms < 2000, `2000 tables listed in ${ms} ms`);
  } finally {
    dropSchema();
  }
}
