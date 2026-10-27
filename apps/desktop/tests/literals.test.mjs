// Unit tests of parameter values written as SQL literals (src/editor/literals.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { literal, substitute } from "../src/editor/literals.ts";

const path = "C:\\temp\\new";

test("numbers, NULL, booleans and quoted text go in as typed", () => {
  for (const driver of ["postgres", "mysql", "bigquery"]) {
    assert.equal(literal(" 42 ", driver), "42");
    assert.equal(literal("-1.5e3", driver), "-1.5e3");
    assert.equal(literal("NULL", driver), "NULL");
    assert.equal(literal("true", driver), "true");
    assert.equal(literal("'as is'", driver), "'as is'");
  }
});

test("backslashes are escaped where literals read them as escapes", () => {
  for (const driver of ["mysql", "clickhouse", "snowflake"]) {
    assert.equal(literal(path, driver), "'C:\\\\temp\\\\new'", driver);
    assert.equal(literal("ends\\", driver), "'ends\\\\'", driver);
  }
  for (const driver of ["bigquery", "databricks"]) {
    assert.equal(literal(path, driver), "'C:\\\\temp\\\\new'", driver);
  }
});

test("backslashes stay where literals take them as they are", () => {
  for (const driver of ["postgres", "sqlite", "duckdb", "oracle", "db2", "trino"]) {
    assert.equal(literal(path, driver), `'${path}'`, driver);
  }
  assert.equal(literal(path, "sqlserver"), `N'${path}'`);
});

test("quotes are doubled, or escaped with a backslash where doubling does not work", () => {
  assert.equal(literal("it's", "postgres"), "'it''s'");
  assert.equal(literal("it's", "mysql"), "'it''s'");
  assert.equal(literal("it's", "sqlserver"), "N'it''s'");
  assert.equal(literal("it's", "bigquery"), "'it\\'s'");
  assert.equal(literal("it's\\", "databricks"), "'it\\'s\\\\'");
});

test("substitute fills placeholders by name and position", () => {
  const sql = "SELECT * FROM t WHERE a = :a AND b = ? AND c = ${col}";
  const spans = [
    { kind: "named", name: "a", start: 26, end: 28 },
    { kind: "positional", name: "?", start: 37, end: 38 },
    { kind: "variable", name: "col", start: 47, end: 53 },
  ];
  assert.equal(
    substitute(sql, spans, { a: path, "?1": "7", col: "x" }, "clickhouse"),
    "SELECT * FROM t WHERE a = 'C:\\\\temp\\\\new' AND b = 7 AND c = x",
  );
});
