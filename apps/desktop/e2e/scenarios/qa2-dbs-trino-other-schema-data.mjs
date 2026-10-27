// DBS-01 (critical, wrong data shown): on Trino, "Open data" on a table of a schema other than
// the connection's shows the rows of the table of the same name in the connection's schema.
// The viewer's query names the table without its schema (`SELECT * FROM "qa_dbs_t"`), so
// Trino resolves it in the session's schema (the connection's `schema` option). When no such
// table exists there, the viewer fails instead; Generate SELECT and the other generated
// statements have the same unqualified name.
//
// Steps: Trino connection on catalog `memory` with schema `qa_dbs` (docker profile trino);
// tables memory.qa_dbs.qa_dbs_t ('from qa_dbs') and memory.default.qa_dbs_t ('from default');
// navigator › default › Tables › qa_dbs_t › Open data.
// Expected: the query names "default"."qa_dbs_t" and the grid shows 'from default'.
// Actual: `SELECT * FROM "qa_dbs_t"`, and the grid shows 'from qa_dbs'.
// Suspected: the ADBC driver marks the wrong schema as current (crates/platypus-driver-adbc/
// src/meta.rs `schemas`, from the driver's CurrentSchema option, which does not follow the
// `schema` option), and apps/desktop/src/sqlNames.ts `qualifiedName` then drops the schema.
import assert from "node:assert/strict";
import { conns, copyAll, idle, needsPorts, openPath, seedTrinoDriver, sh, shotter, stubClipboard, visibleTab } from "../qa-dbs.mjs";
import { menu } from "../qa-xfer.mjs";

export const skip = needsPorts(58085);

export function setup({ dir }) {
  sh("trino", `DROP TABLE IF EXISTS memory.default.qa_dbs_t; DROP SCHEMA IF EXISTS memory.qa_dbs CASCADE; CREATE SCHEMA memory.qa_dbs;
CREATE TABLE memory.qa_dbs.qa_dbs_t (src varchar); INSERT INTO memory.qa_dbs.qa_dbs_t VALUES ('from qa_dbs');
CREATE TABLE memory.default.qa_dbs_t (src varchar); INSERT INTO memory.default.qa_dbs_t VALUES ('from default');`);
  seedTrinoDriver(dir);
  conns(dir, [["QA Trino", "trino"]]);
}

export default async function (page, { shots }) {
  try {
    await openPath(page, ["QA Trino", "default", "Tables", "qa_dbs_t"]);
    await stubClipboard(page);
    await menu(page, "qa_dbs_t", "Open data", null, ["QA Trino", "default", "Tables"]);
    await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null`, 15_000);
    await idle(page, "s.includes('row') || s.includes('Error')", 30_000);
    await shotter(page, shots, "trino-other-schema")("data");
    const sql = await page.eval(`${visibleTab}.querySelector('.cm-content').textContent`);
    assert.match(sql, /"?default"?\."?qa_dbs_t"?/, `the query names the schema: ${sql}`);
    assert.equal(await copyAll(page), "from default");
  } finally {
    sh("trino", "DROP TABLE IF EXISTS memory.default.qa_dbs_t; DROP SCHEMA IF EXISTS memory.qa_dbs CASCADE;");
  }
}
