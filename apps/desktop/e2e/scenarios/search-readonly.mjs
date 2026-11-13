// An Elasticsearch connection saved as read-only: the editor runs requests and SQL that read
// (searches, counts, `_mget`, `_cat`, `HEAD`, a `_sql` body that selects, SELECT) and refuses
// before they run those that write documents, change indices, aliases or cluster settings, a
// `_bulk`, and anything it cannot tell is a read; the data viewer's grid is read-only. The
// documents and indices stay as they were.
// Skipped when Elasticsearch is not running.
import assert from "node:assert/strict";
import { gridTsv, runStatement, treeRow, visibleTab } from "../editor-helpers.mjs";
import { openEditor, runOrGuard } from "../safety-helpers.mjs";
import { connection, countRow, drop, editBar, es, saveConnections, seed, skip as skipSearch, sources } from "../search-helpers.mjs";

const NAME = "E2E ES read-only";

export const skip = skipSearch;
export const setup = async ({ dir }) => {
  saveConnections(dir, connection(60, NAME, "read_only = true\n"));
  await seed("e2e_ro", { properties: { n: { type: "long" } } }, [1, 2, 3].map((n) => [`r${n}`, { n }]));
};

export default async function (page) {
  const before = await sources("e2e_ro");
  await openEditor(page, NAME);
  const writes = [
    'POST /e2e_ro/_doc\n{"n":9}',
    'PUT /e2e_ro/_doc/x\n{"n":9}',
    'POST /e2e_ro/_update/r1\n{"doc":{"n":9}}',
    "DELETE /e2e_ro/_doc/r1",
    "POST /e2e_ro/_delete_by_query",
    'POST /e2e_ro/_delete_by_query\n{"query":{"term":{"n":1}}}',
    'POST /e2e_ro/_update_by_query\n{"script":"ctx._source.n = 0"}',
    'POST /_bulk\n{"delete":{"_index":"e2e_ro","_id":"r1"}}',
    "PUT /e2e_ro_new",
    "DELETE /e2e_ro",
    "DELETE /_all",
    'PUT /e2e_ro/_mapping\n{"properties":{"extra":{"type":"keyword"}}}',
    'POST /_aliases\n{"actions":[{"add":{"index":"e2e_ro","alias":"e2e_ro_alias"}}]}',
    'PUT /_cluster/settings\n{"transient":{"cluster.routing.allocation.enable":"all"}}',
    "POST /e2e_ro/_close",
    "POST /e2e_ro/_forcemerge",
    "POST /_cache/clear",
  ];
  for (const request of writes) {
    const r = await runOrGuard(page, request, { readOnly: true });
    assert.match(r, /^read-only: .*read-only/, `${request}: ${r}`);
  }
  const reads = [
    ["GET /e2e_ro/_search", "3 rows"],
    ['POST /e2e_ro/_search\n{"query":{"term":{"n":2}}}', "1 row"],
    ["POST /e2e_ro/_count", "1 row"],
    ["GET /e2e_ro/_doc/r1", "1 row"],
    ['POST /e2e_ro/_mget\n{"ids":["r1","r2"]}', "2 rows"],
    ["HEAD /e2e_ro", "1 row"],
    ["GET /_cat/indices/e2e_ro", "1 row"],
    ["GET /_cluster/health", "1 row"],
    ["SELECT n FROM e2e_ro ORDER BY n", "3 rows"],
    ["SHOW TABLES LIKE 'e2e_ro'", "1 row"],
    // A raw request to the SQL endpoint is a REST call: its reply is one row (the typed rows
    // are those of an SQL statement, run above).
    ['POST /_sql?format=json\n{"query":"SELECT n FROM e2e_ro ORDER BY n"}', "1 row"],
  ];
  for (const [request, rows] of reads) assert.equal(await runStatement(page, request), rows, request);
  assert.equal(await runStatement(page, "POST /e2e_ro/_count"), "1 row");
  assert.equal(await countRow(page), "3");

  // Nothing changed on the server.
  assert.deepEqual(await sources("e2e_ro"), before);
  assert.equal(await es("HEAD", "/e2e_ro_new", undefined, { allow: [404] }), null);
  assert.deepEqual(Object.keys((await es("GET", "/e2e_ro/_mapping")).e2e_ro.mappings.properties), ["n"]);

  // The data viewer opens read-only.
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("e2e_ro")} !== undefined`, 20_000);
  await page.eval(`${treeRow("e2e_ro")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${editBar}.startsWith('Read-only')`, 15_000);
  assert.ok(await page.eval(`${visibleTab}.querySelector('.edit-bar button[aria-label="Preview SQL"]') === null`));
  await drop("e2e_ro");
}
