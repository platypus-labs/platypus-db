// DBS-18 (low): on Trino, "Generate DDL" (and the DDL section of a table's details) shows only
// "unsupported: object DDL", although Trino answers SHOW CREATE TABLE / SHOW CREATE VIEW for
// every catalog; DBeaver shows it. The same holds for the other ADBC databases that have such
// a statement.
//
// Steps: Trino (docker profile trino), memory.qa_dbs.g (id bigint, v varchar(10));
// navigator › g › Generate DDL.
// Expected: an editor with CREATE TABLE memory.qa_dbs.g (id bigint, v varchar(10)) ….
// Actual: a notice "unsupported: object DDL"; no editor opens.
// Suspected: crates/platypus-driver-adbc/src/lib.rs (object DDL not implemented).
import assert from "node:assert/strict";
import { conns, needsPorts, openPath, seedTrinoDriver, sh, visibleTab } from "../drivers-helpers.mjs";
import { menu } from "../transfer-helpers.mjs";

export const skip = needsPorts(58085);

export function setup({ dir }) {
  sh("trino", "DROP SCHEMA IF EXISTS memory.qa_dbs CASCADE; CREATE SCHEMA memory.qa_dbs; CREATE TABLE memory.qa_dbs.g (id bigint, v varchar(10));");
  seedTrinoDriver(dir);
  conns(dir, [["QA Trino", "trino"]]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA Trino", "qa_dbs", "Tables", "g"]);
    await menu(page, "g", "Generate DDL", null, ["QA Trino", "qa_dbs", "Tables"]);
    const text = `[...document.querySelectorAll('.notice, .toast')].map(n => n.textContent).join(' | ') + ' || ' + (${visibleTab}?.querySelector('.cm-content')?.textContent ?? '')`;
    await page.waitFor(`(${text}).includes('CREATE TABLE') || (${text}).includes('unsupported')`, 15_000);
    assert.match(await page.eval(text), /CREATE TABLE/);
  } finally {
    sh("trino", "DROP SCHEMA IF EXISTS memory.qa_dbs CASCADE;");
  }
}
