// Unit test of the DuckDB viewer's ORDER BY (keyOrderClause in src/sqlIdent.ts): an edited row
// keeps its place with a composite key and, without a key, by `rowid`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { keyOrderClause } from "../src/sqlIdent.ts";

test("keyOrderClause keeps an edited DuckDB row in place", () => {
  assert.equal(keyOrderClause("duckdb", ["id"], true), "ORDER BY id");
  assert.equal(keyOrderClause("duckdb", ["a", "b c"], true), 'ORDER BY a, "b c"');
  assert.equal(keyOrderClause("duckdb", [], true), "ORDER BY rowid");
  assert.equal(keyOrderClause("duckdb", [], false), null);
});
