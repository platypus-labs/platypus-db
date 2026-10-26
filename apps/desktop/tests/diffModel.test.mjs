// Unit tests of "Compare with previous run" (src/analysis/diffModel.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { normalizeSql, previousOf, sameColumns, sameStatement, summary } from "../src/analysis/diffModel.ts";

test("the same statement whatever the spacing and a final semicolon", () => {
  assert.equal(normalizeSql("  SELECT *\n  FROM t ; "), "SELECT * FROM t");
  assert.ok(sameStatement("SELECT * FROM t", "SELECT  *  FROM t;"));
  assert.ok(!sameStatement("SELECT * FROM t", "SELECT * FROM u"));
});

test("columns are the same by name and type", () => {
  const a = [{ name: "id", db_type: "integer" }];
  assert.ok(sameColumns(a, [{ name: "id", db_type: "integer" }]));
  assert.ok(!sameColumns(a, [{ name: "id", db_type: "text" }]));
  assert.ok(!sameColumns(a, [...a, { name: "x", db_type: "text" }]));
});

test("the previous result is one with rows of the same statement", () => {
  const columns = [{ name: "id", db_type: "integer" }];
  const results = [
    { sql: "SELECT 1", jobId: 3, columns, status: { kind: "rows" } },
    { sql: "SELECT * FROM t", jobId: null, columns: null, status: { kind: "error" } },
    { sql: "SELECT * FROM t;", jobId: 7, columns, status: { kind: "rows" } },
  ];
  assert.deepEqual(previousOf(results, "SELECT * FROM t"), { sql: "SELECT * FROM t;", jobId: 7, columns });
  assert.equal(previousOf(results, "SELECT 2"), null);
});

test("the summary counts added, removed and changed rows", () => {
  assert.equal(summary({ added: 12, removed: 3, changed: 5 }), "+12 −3 ~5");
});
