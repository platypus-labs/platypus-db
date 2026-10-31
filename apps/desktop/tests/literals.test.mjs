// Unit tests of parameter values written as SQL literals (src/editor/literals.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { literal, originalOffset, parameterKeys, substitute, utf16Offset } from "../src/editor/literals.ts";

const path = "C:\\temp\\new";

test("numbers, NULL, booleans and quoted text go in as typed", () => {
  for (const driver of ["postgres", "mysql", "bigquery"]) {
    assert.equal(literal(" 42 ", driver), "42");
    assert.equal(literal("0", driver), "0");
    assert.equal(literal("0.5", driver), "0.5");
    assert.equal(literal(".5", driver), ".5");
    assert.equal(literal("1e3", driver), "1e3");
    assert.equal(literal("NULL", driver), "NULL");
    assert.equal(literal("true", driver), "true");
    assert.equal(literal("'as is'", driver), "'as is'");
  }
});

test("a negative number goes in parentheses", () => {
  assert.equal(literal("-1.5e3", "postgres"), "(-1.5e3)");
  assert.equal(literal(" -5 ", "sqlite"), "(-5)");
  assert.equal(literal("-0.5", "sqlite"), "(-0.5)");
});

test("codes that a number would change are strings", () => {
  assert.equal(literal("007", "sqlite"), "'007'");
  assert.equal(literal("0042", "postgres"), "'0042'");
  assert.equal(literal("-007", "postgres"), "'-007'");
  assert.equal(literal("+39333123456", "mysql"), "'+39333123456'");
  assert.equal(literal("00.5", "sqlite"), "'00.5'");
  assert.equal(literal("007", "sqlserver"), "N'007'");
});

test("a value never opens a comment with the text around it", () => {
  const named = "SELECT 10-:p AS r";
  assert.equal(substitute(named, [{ kind: "named", name: "p", start: 10, end: 12 }], { p: "-5" }, "sqlite"), "SELECT 10-(-5) AS r");
  const variable = "SELECT 10-${v} AS r";
  const spans = [{ kind: "variable", name: "v", start: 10, end: 14 }];
  const sent = substitute(variable, spans, { v: "-5" }, "sqlite");
  assert.equal(sent, "SELECT 10- -5 AS r");
  assert.equal(originalOffset(variable, spans, { v: "-5" }, "sqlite", sent.indexOf("AS")), variable.indexOf("AS"));
  assert.equal(originalOffset(variable, spans, { v: "-5" }, "sqlite", 10), 10, "the space belongs to the value");
  const slash = "SELECT 6/${v}";
  assert.equal(substitute(slash, [{ kind: "variable", name: "v", start: 9, end: 13 }], { v: "*x*/2" }, "sqlite"), "SELECT 6/ *x*/2");
  const after = "SELECT ${v}-1";
  assert.equal(substitute(after, [{ kind: "variable", name: "v", start: 7, end: 11 }], { v: "3-" }, "sqlite"), "SELECT 3- -1");
});

test("SQLite's numbered ?1 shares its key with the same number; a plain ? comes after", () => {
  const sql = "SELECT ?1, ?2, ?1, ?";
  const spans = [
    { kind: "named", name: "?1", start: 7, end: 9 },
    { kind: "named", name: "?2", start: 11, end: 13 },
    { kind: "named", name: "?1", start: 15, end: 17 },
    { kind: "positional", name: "", start: 19, end: 20 },
  ];
  assert.deepEqual(parameterKeys(spans), ["?1", "?2", "?1", "?3"]);
  assert.equal(substitute(sql, spans, { "?1": "1", "?2": "2", "?3": "x" }, "sqlite"), "SELECT 1, 2, 1, 'x'");
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

test("a position in the filled-in SQL maps back to the editor's text", () => {
  const sql = "SELECT :p AS a, nosuch FROM t";
  const spans = [{ kind: "named", name: "p", start: 7, end: 9 }];
  const values = { p: "a much longer value" };
  const sent = substitute(sql, spans, values, "sqlite");
  const at = sent.indexOf("nosuch");
  assert.equal(originalOffset(sql, spans, values, "sqlite", at), sql.indexOf("nosuch"));
  assert.equal(originalOffset(sql, spans, values, "sqlite", 3), 3, "before the value");
  assert.equal(originalOffset(sql, spans, values, "sqlite", 10), 7, "inside the value");
  assert.equal(originalOffset(sql, [], {}, "sqlite", 5), 5);
});

test("server positions count characters, JavaScript counts UTF-16 units", () => {
  const sql = "SELECT '\u{1F600}\u{1F600}', nosuch FROM t";
  const chars = [...sql].indexOf("n");
  assert.equal(utf16Offset(sql, chars), sql.indexOf("nosuch"));
  assert.equal(utf16Offset("abc", 2), 2);
  assert.equal(utf16Offset("ab", 5), 5, "past the end");
});
