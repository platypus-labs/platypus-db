// Unit tests of the statements the app writes for Elasticsearch indices (src/sqlNames.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { searchRequest } from "../src/searchSql.ts";
import { quoteIdent } from "../src/sqlIdent.ts";

test("an index is read with a search of all its documents", () => {
  assert.equal(searchRequest("orders", null), 'GET /orders/_search\n{"query":{"match_all":{}}}');
  assert.equal(searchRequest("orders", 50), 'GET /orders/_search\n{"size":50,"query":{"match_all":{}}}');
});

test("the viewer's filter is a query string", () => {
  assert.equal(
    searchRequest("orders", null, ' status:"open" AND n:[1 TO 5] '),
    'GET /orders/_search\n{"query":{"query_string":{"query":"status:\\"open\\" AND n:[1 TO 5]"}}}',
  );
});

test("an index name needs no quoting", () => {
  assert.equal(quoteIdent("logs-2024.01", "elasticsearch"), "logs-2024.01");
  assert.equal(quoteIdent("Orders", "elasticsearch"), "Orders");
});
