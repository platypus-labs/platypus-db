// DBS-13 (high): on Trino the navigator's Rename…, Truncate… and Drop… fail: the dialog runs
// its statement with the trailing ";" it shows (`ALTER TABLE "g" RENAME TO "g2";`), and Trino
// (through its ADBC driver) rejects it: "USER_ERROR: line 1:31: mismatched input ';'.
// Expecting: '.', <EOF>". The statement also names the table without its schema, so it would
// act on the session schema's table of that name (see DBS-01).
//
// Steps: Trino (docker profile trino), memory.qa_dbs.g; navigator › g › Rename…, new name g2,
// Rename; then Truncate… › Truncate; then Drop… › Drop.
// Expected: the table renamed, emptied, dropped.
// Actual: each dialog stays open with the "mismatched input ';'" error; nothing changes.
// Suspected: the object action dialog (apps/desktop/src/components/SqlActionDialog or the
// command running generated statements) sends the previewed script as is; the editor's
// statement splitter (crates/platypus-sql/src/lib.rs `split`) is not used, or the ADBC
// session (crates/platypus-driver-adbc/src/session.rs) does not strip the terminator.
import assert from "node:assert/strict";
import { conns, needsPorts, openPath, seedTrinoDriver, sh } from "../drivers-helpers.mjs";
import { fill, menu } from "../transfer-helpers.mjs";

export const skip = needsPorts(58085);

export function setup({ dir }) {
  sh("trino", "DROP SCHEMA IF EXISTS memory.qa_dbs CASCADE; CREATE SCHEMA memory.qa_dbs; CREATE TABLE memory.qa_dbs.g (id bigint); INSERT INTO memory.qa_dbs.g VALUES (1);");
  seedTrinoDriver(dir);
  conns(dir, [["QA Trino", "trino"]]);
}

const modal = `document.querySelector('.modal.sql-action')`;

/** Runs the navigator action `item` on table `table` (typing `name` when it asks for one) and
 *  returns the dialog's error, or null once it closed. */
async function act(page, table, item, name) {
  await menu(page, table, item, null, ["QA Trino", "qa_dbs", "Tables"]);
  await page.waitFor(`${modal}?.querySelector('pre') != null`, 10_000);
  if (name) {
    await page.eval(fill(`${modal}.querySelector('input')`, name));
    await page.waitFor(`${modal}.querySelector('pre').textContent.includes(${JSON.stringify(name)})`);
  }
  await page.waitFor(`${modal}.querySelector('.actions button.primary')?.disabled === false`, 20_000);
  await page.eval(`${modal}.querySelector('.actions button.primary').click()`);
  await page.waitFor(`${modal} === null || ${modal}.querySelector('.notice.error') !== null`, 20_000);
  return page.eval(`${modal}?.querySelector('.notice.error')?.textContent ?? null`);
}

export default async function (page) {
  try {
    await openPath(page, ["QA Trino", "qa_dbs", "Tables", "g"]);
    assert.equal(await act(page, "g", "Truncate"), null);
    assert.equal(sh("trino", "SELECT count(*) FROM memory.qa_dbs.g;"), "0");
    assert.equal(await act(page, "g", "Rename", "g2"), null);
    assert.equal(sh("trino", "SHOW TABLES FROM memory.qa_dbs;"), "g2");
  } finally {
    sh("trino", "DROP SCHEMA IF EXISTS memory.qa_dbs CASCADE;");
  }
}
