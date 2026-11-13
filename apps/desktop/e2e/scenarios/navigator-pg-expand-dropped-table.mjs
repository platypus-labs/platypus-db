// Bug NAV (low): expanding a table that was dropped since the schema was listed (by another
// session) shows the table as open with nothing below it — no error, no "not found" — while
// Open details of the same row says `qa_nav_dropped qa_nav_gone not found` (also missing the
// dot between schema and name). After the table is created again, closing and reopening the
// row still shows nothing: the empty result is cached until a manual Refresh.
//
// Cause: the details of a missing relation load as empty instead of an error
// (`table_details` of the PostgreSQL driver), and `toggle` in
// apps/desktop/src/components/Navigator.tsx reloads only nodes not in the cache.
//
// Steps: expand the schema; drop qa_nav_gone from psql; expand qa_nav_gone in the navigator.
// Expected: an error row (table not found). Actual: an empty expanded node.
import assert from "node:assert/strict";
import { DBS, connectTo, expand, needs, row, saveConnections } from "../navigator-helpers.mjs";

export const skip = needs("postgres");

const SCHEMA = "qa_nav_dropped";

export function setup({ dir }) {
  DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE; CREATE SCHEMA ${SCHEMA}; CREATE TABLE ${SCHEMA}.qa_nav_gone (id int);`);
  saveConnections(dir, DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a0016"));
}

export default async function (page) {
  try {
    await connectTo(page, "QA PG", "platypus");
    await expand(page, SCHEMA, "qa_nav_gone");
    DBS.postgres.exec(`DROP TABLE ${SCHEMA}.qa_nav_gone;`);
    const rowsBefore = await page.eval(`document.querySelectorAll('.tree-row').length`);
    await page.eval(`${row("qa_nav_gone")}.querySelector('.twisty').click()`);
    await page.waitFor(`${row("qa_nav_gone")}.getAttribute('aria-expanded') === 'true'`);
    // Give the load time to end.
    await page.waitFor(`![...document.querySelectorAll('.tree-row')].some(r => r.textContent.includes('loading…'))`, 15_000);
    await page.sleep(1500);
    const failed = await page.eval(`${row("qa_nav_gone")}.classList.contains('failed') || document.querySelectorAll('.tree-row').length > ${rowsBefore}`);
    assert.ok(failed, `the dropped table shows an error: ${await page.eval(`${row("qa_nav_gone")}.outerHTML.slice(0, 300)`)}`);
  } finally {
    DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE;`);
  }
}
