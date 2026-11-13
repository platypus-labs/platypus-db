// BUG (high, slow navigator): expanding a PostgreSQL schema with 5,000 tables takes ~12.6 s
// (re-expanding from the cache: 0.5 s). The objects query (objects() in
// crates/platypus-driver-pg/src/meta.rs, the size column) runs, for every table,
//   (SELECT sum(r.relpages) FROM pg_class r WHERE r.oid = c.oid OR r.oid = c.reltoastrelid
//      OR r.oid IN (SELECT indexrelid FROM pg_index WHERE indrelid = c.oid))
// whose ORs make PostgreSQL scan all of pg_class per table: quadratic, 8.6 s of the 12.6 s for
// 5,000 tables (20,000 rows in pg_class). Summing relpages of the table, its TOAST table and
// its indexes through joins (or pg_index alone) keeps it linear. DBeaver lists 5,000 tables in
// about a second.
//   Steps: a schema with 5,000 tables; expand it in the navigator.
//   Expected: its folders show within 3 s.  Actual: ~12 s, the twisty spinning.
// Needs PostgreSQL (docker compose -f docker/compose.yml up -d).
import assert from "node:assert/strict";
import { PG, expand, needsPort, openEditor, pgEntry, run, saveConnections, treeRow } from "../perf.mjs";

export const skip = needsPort(PG.port, "PostgreSQL");
const SCHEMA = "qa_perf_nav5k";
const TABLES = 5000;
const LIMIT_MS = 3000;

export function setup({ dir }) {
  saveConnections(dir, pgEntry());
}

/** Many CREATE/DROP TABLE in one transaction overflow the lock table: commit every 500. */
const each = (statement) =>
  `DO $$ BEGIN FOR i IN 1..${TABLES} LOOP EXECUTE format('${statement}', i); IF i % 500 = 0 THEN COMMIT; END IF; END LOOP; END $$`;

export default async function (page) {
  await expand(page, PG.name, PG.password);
  await page.waitFor(`${treeRow("public")} !== undefined`, 20_000);
  await openEditor(page, PG.name);
  await run(page, `DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE`);
  await run(page, `CREATE SCHEMA ${SCHEMA}`);
  await run(page, each(`CREATE TABLE ${SCHEMA}.t_%s (id int PRIMARY KEY, name text, value numeric)`), { timeout: 180_000 });
  try {
    // The navigator lists the new schema after a refresh of the database.
    const database = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === 'platypus' && r.getAttribute('aria-level') === '2')`;
    await page.eval(`${database}.id = 'qa-db'`);
    const r = await page.rect("#qa-db");
    await page.click(r.x + 40, r.y + r.h / 2, { button: "right" });
    await page.waitFor(`document.querySelector('.context-menu') !== null`);
    await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Refresh')).click()`);
    await page.waitFor(`${treeRow(SCHEMA)} !== undefined`, 30_000);
    await page.eval(`${treeRow(SCHEMA)}.id = 'qa-schema'; document.getElementById('qa-schema').scrollIntoView({ block: 'start' })`);
    const started = performance.now();
    await page.clickSel("#qa-schema .twisty");
    await page.waitFor(`[...document.querySelectorAll('.tree-row')].some(r => /^Tables\\s*5[.,]?000$/.test(r.textContent.trim()))`, 120_000);
    const took = performance.now() - started;
    console.log(`  expanding a schema of ${TABLES} tables: ${Math.round(took)} ms`);
    assert.ok(took < LIMIT_MS, `expanding the schema took ${Math.round(took)} ms`);
  } finally {
    await run(page, each(`DROP TABLE IF EXISTS ${SCHEMA}.t_%s`), { timeout: 180_000 });
    await run(page, `DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE`);
  }
}
