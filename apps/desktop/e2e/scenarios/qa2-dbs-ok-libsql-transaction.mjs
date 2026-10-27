// Regression guard (passes): on a libSQL server (over Hrana) a transaction opened in the editor
// spans statements: BEGIN + INSERT as a script, the next statement sees the row, the server's
// other sessions do not, and ROLLBACK undoes it. A wrong table name reports the server's error.
import assert from "node:assert/strict";
import { conns, libsql, needsPorts, openEditor, rows, run, stubClipboard, visibleTab } from "../qa-dbs.mjs";

export const skip = needsPorts(58080);

export function setup({ dir }) {
  libsql("DROP TABLE IF EXISTS qa_dbs_tx", "CREATE TABLE qa_dbs_tx (id INTEGER PRIMARY KEY, v TEXT)", "INSERT INTO qa_dbs_tx VALUES (1, 'a')");
  conns(dir, [["QA libSQL", "libsql"]]);
}

export default async function (page) {
  try {
    await openEditor(page, "QA libSQL", null);
    await stubClipboard(page);
    await run(page, "BEGIN; INSERT INTO qa_dbs_tx VALUES (2, 'b');", true);
    await page.waitFor(`${visibleTab}.querySelector('.toolbar').textContent.includes('Transaction open')`, 10_000);
    assert.deepEqual(libsql("SELECT count(*) FROM qa_dbs_tx"), [[1]]);
    await run(page, "SELECT count(*) AS n FROM qa_dbs_tx");
    assert.deepEqual(await rows(page), [["2"]]);
    await run(page, "ROLLBACK");
    await run(page, "SELECT count(*) AS n FROM qa_dbs_tx");
    assert.deepEqual(await rows(page), [["1"]]);
    await run(page, "SELECT * FROM qa_dbs_nope");
    assert.match(await page.eval(`${visibleTab}.querySelector('.result-error')?.textContent ?? ''`), /no such table: qa_dbs_nope/);
  } finally {
    libsql("DROP TABLE IF EXISTS qa_dbs_tx");
  }
}
