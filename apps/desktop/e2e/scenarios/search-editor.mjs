// Elasticsearch from the `search` profile of docker/compose.yml, in the editor: console
// requests with bodies (a search, a count, writes by id, an update, a delete), NDJSON bodies
// (`_bulk` with and without a failing item, `_msearch`), several requests of a script, comments,
// the statement under the cursor, `_cat` and `HEAD`, aggregations as results of their own, SQL
// with typed columns and its cursor paging, highlighting that follows the kind of statement,
// completion of the index names, their fields and the APIs, server errors with their reason,
// a syntax error in a JSON body, and Cancel on a slow search. The session goes on after every
// error.
// Skipped when Elasticsearch is not running.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import {
  button,
  clearStatus,
  editorText,
  gridTsv,
  idle,
  openEditor,
  resultError,
  resultTabs,
  runScript,
  runStatement,
  setEditor,
  status,
  visibleTab,
} from "../qa-sql.mjs";
import { connection, drop, es, saveConnections, seed, skip as skipSearch, sources, table } from "../search-helpers.mjs";

const NAME = "E2E ES editor";

export const skip = skipSearch;
export const setup = async ({ dir }) => {
  saveConnections(dir, connection(30, NAME));
  const mapping = {
    properties: {
      n: { type: "long" },
      name: { type: "keyword" },
      price: { type: "double" },
      ok: { type: "boolean" },
      born: { type: "date" },
      status: { type: "keyword" },
    },
  };
  await seed(
    "e2e_ed",
    mapping,
    [1, 2, 3, 4, 5].map((n) => [`s${n}`, { n, name: `item ${n}`, price: n + 0.5, ok: n % 2 === 0, born: "2024-05-06T07:08:09Z", status: n <= 2 ? "open" : "closed" }]),
  );
  await seed("e2e_sqlbig", { properties: { n: { type: "long" } } }, Array.from({ length: 2500 }, (_, i) => ({ n: i + 1 })));
  await seed("e2e_slow", { properties: { n: { type: "long" } } }, Array.from({ length: 4000 }, (_, i) => ({ n: i + 1 })));
};

/** Runs and expects an error whose text matches `re`. */
async function fails(page, text, re) {
  assert.equal(await runStatement(page, text), "Error", text);
  assert.match(await page.eval(resultError), re, text);
}

/** Types `text` into the editor and returns whether the completion list offers `label`. */
async function offers(page, text, label) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(text);
  await page.waitFor(`${editorText} === ${JSON.stringify(text)}`, 10_000);
  const list = `[...document.querySelectorAll('.cm-tooltip-autocomplete li')].map(li => li.textContent)`;
  let found = false;
  // The catalog loads in the background after the tab opens: ask again for a few seconds.
  for (let i = 0; i < 20 && !found; i++) {
    await page.key(" ", "Space", 32, CTRL);
    await page.sleep(400);
    found = (await page.eval(list)).some((entry) => entry.startsWith(label));
  }
  await page.key("Escape", "Escape", 27);
  await page.sleep(150);
  return found;
}

/** The colours of the spans of editor line `n`, and the colour of plain text there. */
const colors = (page, n) =>
  page.eval(`(() => {
    const line = ${visibleTab}.querySelector('.cm-content').querySelectorAll('.cm-line')[${n}];
    return { base: getComputedStyle(line).color, spans: [...line.querySelectorAll('span')].map(s => [s.textContent, getComputedStyle(s).color]) };
  })()`);

export default async function (page) {
  await openEditor(page, NAME);

  // A search with a body.
  assert.equal(await runStatement(page, 'GET /e2e_ed/_search\n{"query":{"term":{"status":"open"}},"sort":[{"n":"asc"}]}'), "2 rows");
  let rows = table(await gridTsv(page));
  assert.deepEqual(rows[0].slice(0, 2), ["_id", "n"]);
  assert.deepEqual(rows.slice(1).map((r) => r[0]), ["s1", "s2"]);
  assert.equal(await runStatement(page, "GET /e2e_ed/_search?q=status:closed"), "3 rows");
  // The slash of the path is optional; a count is one row of one column.
  assert.equal(await runStatement(page, "GET e2e_ed/_count"), "1 row");
  assert.equal(await gridTsv(page), "count\n5");
  // HEAD tells whether an index exists, as a row.
  assert.equal(await runStatement(page, "HEAD /e2e_ed"), "1 row");
  assert.equal(await gridTsv(page), "exists\tstatus\ntrue\t200");
  assert.equal(await runStatement(page, "HEAD /e2e_no_such"), "1 row");
  assert.equal(await gridTsv(page), "exists\tstatus\nfalse\t404");
  // `_cat` answers a table.
  assert.equal(await runStatement(page, "GET /_cat/indices/e2e_ed"), "1 row");
  assert.match(await gridTsv(page), /e2e_ed/);

  // Writes give the number of documents they changed, and really change them.
  assert.match(await runStatement(page, 'POST /e2e_ed/_doc?refresh=true\n{"n":6,"name":"six","status":"open"}'), /^1 rows? affected/);
  assert.match(await runStatement(page, 'PUT /e2e_ed/_doc/fixed?refresh=true\n{"n":7,"name":"seven"}'), /^1 rows? affected/);
  assert.equal((await sources("e2e_ed")).fixed.name, "seven");
  assert.match(await runStatement(page, 'POST /e2e_ed/_update/fixed?refresh=true\n{"doc":{"n":77}}'), /^1 rows? affected/);
  assert.equal((await sources("e2e_ed")).fixed.n, 77);
  assert.match(await runStatement(page, "DELETE /e2e_ed/_doc/fixed?refresh=true"), /^1 rows? affected/);
  assert.equal((await sources("e2e_ed")).fixed, undefined);
  assert.match(await runStatement(page, "DELETE /e2e_ed/_doc/nope?refresh=true"), /^0 rows? affected/);
  assert.match(await runStatement(page, 'POST /e2e_ed/_delete_by_query?refresh=true\n{"query":{"term":{"n":6}}}'), /^1 rows? affected/);
  assert.equal(Object.keys(await sources("e2e_ed")).length, 5);

  // NDJSON: a `_bulk` of lines, ended by the end of the text.
  const bulk = '{"index":{"_index":"e2e_ed","_id":"b1"}}\n{"n":101,"name":"bulk one"}\n{"index":{"_index":"e2e_ed","_id":"b2"}}\n{"n":102,"name":"bulk two"}';
  assert.match(await runStatement(page, `POST /_bulk?refresh=true\n${bulk}`), /^2 rows? affected/);
  assert.equal((await sources("e2e_ed")).b2.name, "bulk two");
  // An item the server refuses: the items are listed, with the reason, and the others applied.
  const mixed = '{"create":{"_index":"e2e_ed","_id":"b1"}}\n{"n":1}\n{"index":{"_index":"e2e_ed","_id":"b3"}}\n{"n":103}';
  assert.equal(await runStatement(page, `POST /_bulk?refresh=true\n${mixed}`), "2 rows");
  const items = await gridTsv(page);
  assert.match(items, /version_conflict_engine_exception/);
  assert.equal((await sources("e2e_ed")).b3.n, 103);
  // `_msearch`: a result per search.
  await runScript(page, 'POST /_msearch\n{"index":"e2e_ed"}\n{"size":1}\n{"index":"e2e_ed"}\n{"size":2,"sort":[{"n":"asc"}]}');
  assert.equal((await page.eval(resultTabs)).length, 2);
  await es("POST", "/e2e_ed/_delete_by_query?refresh=true", { query: { ids: { values: ["b1", "b2", "b3"] } } });

  // Aggregations come after the hits, one result each.
  await runScript(page, 'POST /e2e_ed/_search\n{"size":0,"aggs":{"top":{"max":{"field":"n"}},"by_status":{"terms":{"field":"status"}}}}');
  assert.equal((await page.eval(resultTabs)).length, 2);

  // A script runs each request, with a result each; blank lines and comments between.
  await runScript(page, "# first\nGET /e2e_ed/_count\n\n// second\nGET /e2e_ed/_search?size=2\n\n-- third, in SQL\nSELECT COUNT(*) AS total FROM e2e_ed");
  assert.equal((await page.eval(resultTabs)).length, 3);

  // Comments before a request are skipped; the statement under the cursor runs.
  assert.equal(await runStatement(page, "# the count\n// again\nGET /e2e_ed/_count"), "1 row");
  assert.equal(await gridTsv(page), "count\n5");
  await setEditor(page, "GET /e2e_ed/_count\n\nGET /e2e_ed/_search?size=2\n\nSELECT COUNT(*) AS total FROM e2e_ed");
  await page.key("Home", "Home", 36, CTRL);
  assert.equal(await runStatement(page), "1 row");
  assert.equal(await gridTsv(page), "count\n5");
  await page.key("ArrowDown", "ArrowDown", 40, 0);
  await page.key("ArrowDown", "ArrowDown", 40, 0);
  assert.equal(await runStatement(page), "2 rows");
  await page.key("End", "End", 35, CTRL);
  assert.equal(await runStatement(page), "1 row");
  assert.match(await gridTsv(page), /^total\n5(\.0)?$/);

  // SQL: typed columns, aggregates, LIMIT, a statement ended by `;`, SHOW TABLES.
  assert.equal(await runStatement(page, "SELECT n, price, name, ok, born FROM e2e_ed ORDER BY n"), "5 rows");
  rows = table(await gridTsv(page));
  assert.deepEqual(rows[0], ["n", "price", "name", "ok", "born"]);
  assert.deepEqual(rows[1].slice(0, 4), ["1", "1.5", "item 1", "false"]);
  assert.deepEqual(rows[2].slice(0, 4), ["2", "2.5", "item 2", "true"]);
  assert.match(rows[1][4], /2024-05-06/);
  assert.equal(await runStatement(page, "SELECT status, COUNT(*) AS c FROM e2e_ed GROUP BY status ORDER BY status;"), "2 rows");
  assert.equal(await gridTsv(page), "status\tc\nclosed\t3\nopen\t2");
  assert.equal(await runStatement(page, "SELECT n FROM e2e_ed ORDER BY n DESC LIMIT 2"), "2 rows");
  assert.equal(await gridTsv(page), "n\n5\n4");
  assert.equal(await runStatement(page, "SELECT n FROM e2e_ed WHERE name LIKE 'item 3'"), "1 row");
  assert.equal(await runStatement(page, "SHOW TABLES LIKE 'e2e_ed'"), "1 row");
  // A cursor brings the rest of a long result: the first 1,000 rows, then all with "Fetch all".
  assert.match(await runStatement(page, "SELECT n FROM e2e_sqlbig ORDER BY n", 60_000), /^1,000 rows loaded · more available/);
  await page.eval(`[...${visibleTab}.querySelectorAll('.statusbar button')].find(b => b.textContent === 'Fetch all').click()`);
  await page.waitFor(`/^2,500 rows/.test(${status}) && !/more available/.test(${status})`, 60_000);
  const long = table(await gridTsv(page));
  assert.equal(long.length, 2501);
  assert.equal(long[1][0], "1");
  assert.equal(long[2500][0], "2500");

  // Errors keep the server's reason; the session goes on.
  await fails(page, "GET /e2e_no_such_index/_search", /no such index/);
  await fails(page, 'POST /e2e_ed/_search\n{"query":{"nope":{}}}', /nope/);
  await fails(page, 'POST /e2e_ed/_doc\n{"n":"not a number"}', /failed to parse|number/);
  await fails(page, "PUT /e2e_ed", /already exists/);
  await fails(page, "SELECT nope FROM e2e_ed", /nope/);
  await fails(page, "SELECT n FROM e2e_no_such_index", /e2e_no_such_index/);
  // A body that is not JSON never reaches the server; the message says which statement.
  await fails(page, 'POST /e2e_ed/_doc\n{"n": }', /invalid JSON body/);
  assert.equal(Object.keys(await sources("e2e_ed")).length, 5, "a refused document was written");
  assert.equal(await runStatement(page, "GET /e2e_ed/_count"), "1 row");
  assert.equal(await gridTsv(page), "count\n5");

  // Highlighting follows the kind of statement: comments, the method and path of a request, the
  // JSON of its body, the keywords of SQL.
  await setEditor(page, '# note\nGET /e2e_ed/_search\n{"size":1,"query":{"term":{"name":"x"}}}\n\nSELECT COUNT(*) FROM e2e_ed');
  const [comment, request, body, , sql] = await Promise.all([0, 1, 2, 3, 4].map((n) => colors(page, n)));
  assert.ok(comment.spans.some(([, c]) => c !== comment.base), `comment line: ${JSON.stringify(comment)}`);
  assert.ok(request.spans.some(([text, c]) => text.includes("GET") && c !== request.base), `request line: ${JSON.stringify(request)}`);
  assert.ok(body.spans.length >= 3 && body.spans.some(([, c]) => c !== body.base), `body line: ${JSON.stringify(body)}`);
  assert.ok(new Set(body.spans.map(([, c]) => c)).size >= 2, `body colours: ${JSON.stringify(body)}`);
  assert.ok(sql.spans.some(([text, c]) => /SELECT|COUNT|FROM/.test(text) && c !== sql.base), `sql line: ${JSON.stringify(sql)}`);

  // Completion: indices after the method, the APIs after an index, the fields of the index in
  // a body, the indices after FROM in SQL.
  assert.ok(await offers(page, "GET /e2e_", "e2e_ed"), "index name after the method");
  assert.ok(await offers(page, "GET /e2e_ed/_se", "_search"), "API after the index");
  assert.ok(await offers(page, 'GET /e2e_ed/_search\n{"query":{"term":{"na', "name"), "field in a body");
  assert.ok(await offers(page, "SELECT * FROM e2e_", "e2e_ed"), "index name after FROM");
  assert.ok(await offers(page, "GE", "GET"), "HTTP method");

  // Cancel stops a search that runs long on the server, and the session goes on.
  const slow =
    'POST /e2e_slow/_search\n{"size":1,"query":{"script":{"script":{"source":"long s = 0; for (long i = 0; i < 900000; i++) { s += i % 7; } return s > 0 && doc[\'n\'].value > 0;"}}}}';
  await setEditor(page, slow);
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${button("Cancel")}?.disabled === false`, 10_000);
  await page.sleep(700);
  await page.eval(`${button("Cancel")}.click()`);
  await page.waitFor(`${idle} && ${status} !== '' && !${status}.includes('Executing')`, 30_000);
  assert.match(`${await page.eval(status)} ${await page.eval(resultError)}`, /cancel/i);
  assert.equal(await runStatement(page, "GET /e2e_slow/_count"), "1 row");
  assert.equal(await gridTsv(page), "count\n4000");
  await drop("e2e_ed", "e2e_sqlbig", "e2e_slow");
}
