// Bug NAV: on PostgreSQL, a function or procedure whose parameters have names cannot be opened:
// Open details shows only `syntax error at or near "integer"` (Properties and DDL), and
// Generate DDL fails with the same error.
//
// The navigator lists routines as `name(identity arguments)`, which PostgreSQL writes with the
// parameter names (`qa_nav_fn(a integer)`); `routine()` in crates/platypus-driver-pg/src/ddl.rs
// casts that text to `regprocedure`, which accepts types only (`qa_nav_fn(integer)`).
// Named parameters are the norm in real schemas, so most routines are affected.
//
// Steps: connect, expand the schema, Functions, right-click `qa_nav_fn(a integer)` › Open details,
// then the DDL section.
// Expected: the properties and `CREATE OR REPLACE FUNCTION …` DDL. Actual: the syntax error.
import assert from "node:assert/strict";
import { DBS, childRow, connectTo, expand, menu, needs, row, saveConnections } from "../qa-nav-helpers.mjs";

export const skip = needs("postgres");

const SCHEMA = "qa_nav_routines";
const objTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;

export function setup({ dir }) {
  DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE;
    CREATE SCHEMA ${SCHEMA};
    CREATE FUNCTION ${SCHEMA}.qa_nav_add(a integer, b integer) RETURNS integer LANGUAGE sql AS 'SELECT a + b';
    CREATE PROCEDURE ${SCHEMA}.qa_nav_log(msg text) LANGUAGE plpgsql AS $$ BEGIN RAISE NOTICE '%', msg; END $$;`);
  saveConnections(dir, DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a0011"));
}

async function details(page, name) {
  await menu(page, name, "Open details");
  await page.waitFor(`${objTab}?.querySelector('.object-title strong')?.textContent === ${JSON.stringify(name)}`, 15_000);
  // Loaded: the properties table or an error.
  await page.waitFor(`${objTab}.querySelector('.meta-table, .notice.error') !== null`, 15_000);
  const error = await page.eval(`${objTab}.querySelector('.notice.error')?.textContent ?? null`);
  assert.equal(error, null, `Open details of ${name}`);
  await page.eval(`[...${objTab}.querySelectorAll('.object-sections button')].find(b => b.textContent === 'DDL').click()`);
  await page.waitFor(`/CREATE OR REPLACE (FUNCTION|PROCEDURE)/.test(${objTab}.querySelector('.object-ddl .cm-content')?.textContent ?? '')`, 15_000);
}

export default async function (page, { shots }) {
  try {
    await connectTo(page, "QA PG", "platypus");
    await expand(page, SCHEMA);
    await page.waitFor(`${childRow(SCHEMA, "Procedures")} !== undefined`, 15_000);
    await page.eval(`${childRow(SCHEMA, "Functions")}.querySelector('.twisty').click()`);
    await page.waitFor(`${childRow(SCHEMA, "Functions")}.getAttribute('aria-expanded') === 'true'`);
    await page.eval(`${childRow(SCHEMA, "Procedures")}.querySelector('.twisty').click()`);
    await page.waitFor(`${row("qa_nav_add(a integer, b integer)")} !== undefined && ${row("qa_nav_log(IN msg text)")} !== undefined`);
    if (shots) await page.screenshot(`${shots}/qa-nav-pg-routine-named-args.png`);
    await details(page, "qa_nav_add(a integer, b integer)");
    await details(page, "qa_nav_log(IN msg text)");
  } finally {
    DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE;`);
  }
}
