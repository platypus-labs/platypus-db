// Unit tests of what the quick filters compare (src/grid/features/filterOperand.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { filterOperand } from "../src/grid/features/filterOperand.ts";

test("types without equality are compared as text or bytes", () => {
  assert.equal(filterOperand("postgres", "json", "doc"), "doc::text");
  assert.equal(filterOperand("postgres", "xml", '"X"'), '"X"::text');
  assert.equal(filterOperand("postgres", "point", "p"), "p::text");
  assert.equal(filterOperand("postgres", "json[]", "j"), "j::text");
  assert.equal(filterOperand("sqlserver", "ntext", "[n]"), "CAST([n] AS nvarchar(max))");
  assert.equal(filterOperand("sqlserver", "text", "[t]"), "CAST([t] AS nvarchar(max))");
  assert.equal(filterOperand("sqlserver", "xml", "[x]"), "CAST([x] AS nvarchar(max))");
  assert.equal(filterOperand("sqlserver", "sql_variant", "[v]"), "CAST([v] AS nvarchar(max))");
  assert.equal(filterOperand("sqlserver", "udt", "[g]"), "CAST([g] AS varbinary(max))");
  assert.equal(filterOperand("sqlserver", "image", "[i]"), "CAST([i] AS varbinary(max))");
  assert.equal(filterOperand("mysql", "JSON", "`j`"), "CAST(`j` AS CHAR)");
});

test("other columns are compared as they are, so that their indexes serve", () => {
  assert.equal(filterOperand("postgres", "jsonb", "doc"), "doc");
  assert.equal(filterOperand("postgres", "text", "t"), "t");
  assert.equal(filterOperand("sqlserver", "nvarchar(20)", "[n]"), "[n]");
  assert.equal(filterOperand("sqlite", "json", "j"), "j");
  assert.equal(filterOperand("duckdb", "JSON", "j"), "j");
});
