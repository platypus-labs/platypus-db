// Elasticsearch from the `search` profile of docker/compose.yml, read past the first page: an
// index of 2,500 documents opens with its first 1,000 and "Fetch all" brings the rest in order
// (a point in time on the server, closed afterwards); a field that only a later document has
// and that the mapping does not know goes to the `_other` column; a value that does not fit
// its mapped type (an array in a long field, text in a long field that ignores malformed
// values) shows as text instead of failing; `size` caps the rows, a `from` is one plain
// request, a sort of the request is kept through the pages.
// Skipped when Elasticsearch is not running.
import assert from "node:assert/strict";
import { gridTsv, openEditor, runStatement, status, treeRow, visibleTab } from "../qa-sql.mjs";
import { connection, drop, expandCluster, openIndex, saveConnections, seed, skip as skipSearch, table } from "../search-helpers.mjs";

const NAME = "E2E ES paging";

export const skip = skipSearch;
export const setup = async ({ dir }) => {
  saveConnections(dir, connection(20, NAME));
  const docs = Array.from({ length: 2500 }, (_, i) => {
    const n = i + 1;
    return [`d${n}`, { n, name: `doc ${n}`, ...(n === 2200 ? { late: { a: 1 } } : {}) }];
  });
  await seed("e2e_page", { dynamic: false, properties: { n: { type: "long" }, name: { type: "keyword" } } }, docs);
  await seed("e2e_mixed", { properties: { n: { type: "long", ignore_malformed: true }, label: { type: "keyword" } } }, [
    ["m1", { n: 1, label: "plain" }],
    ["m2", { n: [7, 8], label: "array" }],
    ["m3", { n: "oops", label: "text" }],
    ["m4", { n: 4, label: "plain" }],
  ]);
};

const fetchAll = (page) =>
  page.eval(`[...${visibleTab}.querySelectorAll('.statusbar button')].find(b => b.textContent === 'Fetch all').click()`);

/** The cells of a column of the shown grid, quotes of the copy removed. */
const column = (rows, name) => {
  const at = rows[0].indexOf(name);
  assert.ok(at >= 0, `${name} in ${rows[0]}`);
  return rows.slice(1).map((r) => r[at].replace(/^"|"$/g, "").replace(/""/g, '"'));
};

export default async function (page) {
  await expandCluster(page, NAME, "e2e_page");

  // The first page, then everything in order with "Fetch all".
  await page.eval(`${treeRow("e2e_page")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`/^1,000 rows loaded · more available/.test(${status})`, 30_000);
  await fetchAll(page);
  await page.waitFor(`/^2,500 rows/.test(${status}) && !/more available/.test(${status})`, 60_000);
  const rows = table(await gridTsv(page));
  assert.deepEqual(rows[0], ["_id", "n", "name", "_other"]);
  assert.equal(rows.length, 2501);
  assert.deepEqual(column(rows, "n").slice(0, 3), ["1", "2", "3"]);
  assert.equal(column(rows, "n")[2499], "2500");
  assert.equal(new Set(column(rows, "_id")).size, 2500, "a document twice");

  // `late` is on a document of the third page and in no mapping: it is in `_other`, as JSON.
  const other = column(rows, "_other");
  assert.equal(other[2199], '{"late":{"a":1}}');
  assert.equal(other.filter((v) => v !== "").length, 1, "only one document has an unmapped field");

  // A value that does not fit the mapped type is text, not an error.
  await openIndex(page, "e2e_mixed", "4 rows");
  const mixed = table(await gridTsv(page));
  const n = Object.fromEntries(column(mixed, "_id").map((id, i) => [id, column(mixed, "n")[i]]));
  assert.deepEqual(n, { m1: "1", m2: "[7,8]", m3: "oops", m4: "4" });

  // The editor: `size` above a page is the most rows read; below, one plain request.
  await openEditor(page, NAME);
  assert.match(await runStatement(page, 'GET /e2e_page/_search\n{"size":1500,"query":{"match_all":{}}}', 60_000), /^1,000 rows loaded · more available/);
  await fetchAll(page);
  await page.waitFor(`/^1,500 rows/.test(${status}) && !/more available/.test(${status})`, 30_000);
  assert.equal(await runStatement(page, 'GET /e2e_page/_search\n{"size":3}'), "3 rows");
  assert.equal(await runStatement(page, "GET /e2e_page/_search?size=2"), "2 rows");
  assert.equal(await runStatement(page, 'GET /e2e_page/_search\n{"from":2495,"size":10}'), "5 rows");
  assert.equal(await runStatement(page, 'GET /e2e_page/_search\n{"size":0}'), "0 rows");

  // A sort in the request is kept through the pages.
  assert.match(await runStatement(page, 'GET /e2e_page/_search\n{"sort":[{"n":"desc"}],"query":{"match_all":{}}}', 60_000), /^1,000 rows loaded/);
  await fetchAll(page);
  await page.waitFor(`/^2,500 rows/.test(${status})`, 60_000);
  const sorted = column(table(await gridTsv(page)), "n");
  assert.equal(sorted[0], "2500");
  assert.equal(sorted[1000], "1500");
  assert.equal(sorted[2499], "1");

  // A filter narrows the pages: 700 documents are one page.
  assert.equal(await runStatement(page, 'GET /e2e_page/_search\n{"query":{"range":{"n":{"lte":700}}}}', 30_000), "700 rows");

  await drop("e2e_page", "e2e_mixed");
}
