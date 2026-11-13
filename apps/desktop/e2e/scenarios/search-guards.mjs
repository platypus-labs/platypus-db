// Elasticsearch requests that change everything ask first, as a DELETE without WHERE does:
// `_delete_by_query` and `_update_by_query` with no query, with `match_all`, with an empty
// query or with `q=*`, and `DELETE /_all` and `DELETE /*`. The question says what the request
// does and how many documents the index has; Cancel keeps the data and Run goes on. A request
// with a real query, a delete by id and every read run without asking.
// Skipped when Elasticsearch is not running.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { clearStatus, gridTsv, openEditor, runStatement, setEditor, status, waitDone } from "../editor-helpers.mjs";
import { runOrGuard } from "../safety-helpers.mjs";
import { connection, countRow, drop, es, saveConnections, seed, skip as skipSearch, sources } from "../search-helpers.mjs";

const NAME = "E2E ES guards";
const DOCS = () => [1, 2, 3, 4, 5].map((n) => [`g${n}`, { n, name: `doc ${n}` }]);
const MAPPING = { properties: { n: { type: "long" }, name: { type: "keyword" } } };

export const skip = skipSearch;
export const setup = async ({ dir }) => {
  saveConnections(dir, connection(50, NAME));
  await seed("e2e_guard", MAPPING, DOCS());
  await seed("e2e_guard_tmp", MAPPING, DOCS().slice(0, 3));
};

const count = async (index) => Object.keys(await sources(index)).length;

/** The question the request raises: it must, and Cancel leaves the documents. */
async function asks(page, request, re) {
  const r = await runOrGuard(page, request);
  assert.match(r, /^guard: /, `no question for ${request}: ${r}`);
  assert.match(r, re, r);
  assert.equal(await count("e2e_guard"), 5, `${request} changed the index`);
}

export default async function (page) {
  await openEditor(page, NAME);

  // Deleting by query without a filter: no body, an empty query, match_all, q=*.
  const every = /DELETE BY QUERY\s*with an empty filter changes every document of\s*e2e_guard.*about 5 documents/s;
  await asks(page, "POST /e2e_guard/_delete_by_query", every);
  await asks(page, 'POST /e2e_guard/_delete_by_query\n{"query":{"match_all":{}}}', every);
  await asks(page, 'POST /e2e_guard/_delete_by_query\n{"query":{}}', every);
  await asks(page, "POST /e2e_guard/_delete_by_query?q=*", every);
  await asks(page, "POST /e2e_guard/_delete_by_query?conflicts=proceed&refresh=true", every);
  await asks(page, "POST /e2e_guard/_update_by_query", /UPDATE BY QUERY\s*with an empty filter changes every document of\s*e2e_guard/);
  await asks(page, 'POST /e2e_guard/_update_by_query\n{"script":{"source":"ctx._source.n = 0"}}', /UPDATE BY QUERY/);

  // Every index of the cluster. (Elasticsearch 8 refuses it by default too: a second wall.)
  await asks(page, "DELETE /_all", /DELETE INDEX\s*deletes every index of the cluster/);
  await asks(page, "DELETE /*", /DELETE INDEX\s*deletes every index of the cluster/);
  assert.equal((await es("GET", "/e2e_guard/_count")).count, 5);

  // A script asks once for all of its requests; Cancel runs none of them.
  await setEditor(page, "GET /e2e_guard/_count\n\nPOST /e2e_guard/_delete_by_query\n\nGET /e2e_guard/_search");
  await clearStatus(page);
  await page.eval(`[...document.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Script').click()`);
  await page.waitFor(`document.querySelector('.modal.guard') !== null`, 10_000);
  assert.match(await page.eval(`document.querySelector('.modal.guard').textContent`), /DELETE BY QUERY/);
  await page.eval(`[...document.querySelectorAll('.modal.guard button')].find(b => b.textContent.trim() === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.modal') === null`, 5_000);
  assert.equal(await count("e2e_guard"), 5);

  // With a real query, by id, and every read: no question.
  assert.match(await runStatement(page, 'POST /e2e_guard/_delete_by_query?refresh=true\n{"query":{"term":{"n":1}}}'), /^1 rows? affected/);
  assert.match(await runStatement(page, "POST /e2e_guard/_delete_by_query?refresh=true&q=n:2"), /^1 rows? affected/);
  assert.match(await runStatement(page, "DELETE /e2e_guard/_doc/g3?refresh=true"), /^1 rows? affected/);
  assert.equal(await runStatement(page, "GET /e2e_guard/_search"), "2 rows");
  assert.equal(await runStatement(page, "SELECT n FROM e2e_guard ORDER BY n"), "2 rows");
  assert.equal(await count("e2e_guard"), 2);

  // Run goes on: the documents of the throwaway index are deleted after the question. (The
  // server wants a query in the body, so the unfiltered one is a match_all.)
  await setEditor(page, 'POST /e2e_guard_tmp/_delete_by_query?refresh=true\n{"query":{"match_all":{}}}');
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`document.querySelector('.modal.guard') !== null`, 10_000);
  assert.match(await page.eval(`document.querySelector('.modal.guard').textContent`), /e2e_guard_tmp.*about 3 documents/s);
  await page.eval(`[...document.querySelectorAll('.modal.guard button')].find(b => b.textContent.trim() === 'Run').click()`);
  assert.match(await waitDone(page, 20_000), /^3 rows? affected/);
  assert.equal(await count("e2e_guard_tmp"), 0);
  assert.match(await page.eval(status), /^3 rows? affected/);
  assert.equal(await runStatement(page, "GET /e2e_guard_tmp/_count"), "1 row");
  assert.equal(await countRow(page), "0");
  await drop("e2e_guard", "e2e_guard_tmp");
}
