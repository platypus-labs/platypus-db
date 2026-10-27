// DBS-xx (high): on a libSQL server (sqld, Turso) a table whose rows exceed the server's
// response limit cannot be opened at all: "Open data" (and any query returning many rows)
// fails with "RESPONSE_TOO_LARGE: Response is too large" and shows no row, while every other
// driver streams the first page of the same table. The driver asks for the whole result in one
// Hrana response instead of reading it in pages (a cursor, or LIMIT/OFFSET pages).
//
// Steps: libSQL connection (docker profile libsql, port 58080), a table of 200 000 rows of
// ~60 bytes each; expand the connection, right click the table > Open data.
// Expected: the first rows are shown ("… rows"), as on SQLite.
// Actual: the result pane shows RESPONSE_TOO_LARGE and the status bar "Error".
// Suspected: crates/platypus-driver-libsql/src/hrana.rs / session.rs (one `execute` request
// per statement, no paging).
import { conns, idle, libsql, needsPorts, openPath, shotter, treeMenu, visibleTab } from "../qa-dbs.mjs";
import assert from "node:assert/strict";

export const skip = needsPorts(58080);

export function setup({ dir }) {
  libsql(
    "DROP TABLE IF EXISTS qa_dbs_big",
    "CREATE TABLE qa_dbs_big (id INTEGER PRIMARY KEY, label TEXT)",
    "INSERT INTO qa_dbs_big WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 200000) SELECT x, 'a label of some fifty characters for row number ' || x FROM c",
  );
  conns(dir, [["QA libSQL", "libsql"]]);
}

export default async function (page, { shots }) {
  try {
    await openPath(page, ["QA libSQL", "main", "Tables", "qa_dbs_big"]);
    await treeMenu(page, "qa_dbs_big", "Open data");
    await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null`, 15_000);
    const status = await idle(page, "true", 60_000);
    await shotter(page, shots, "libsql-large")("result");
    const error = await page.eval(`${visibleTab}.querySelector('.result-error')?.textContent ?? null`);
    assert.equal(error, null, `the table opens: ${error}`);
    assert.match(status, /rows/);
  } finally {
    libsql("DROP TABLE IF EXISTS qa_dbs_big");
  }
}
