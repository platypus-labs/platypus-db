// Unit tests of how a selection runs (src/components/selectionRun.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { selectionAsScript } from "../src/components/selectionRun.ts";

test("one statement runs as typed everywhere", () => {
  for (const driver of ["postgres", "sqlite", "clickhouse", "sqlserver", "mysql"]) {
    assert.equal(selectionAsScript(["query"], driver), false, driver);
  }
});

test("several statements run one by one where the driver does not batch them", () => {
  for (const driver of ["postgres", "sqlite", "clickhouse", "duckdb", "libsql", "oracle", "snowflake"]) {
    assert.equal(selectionAsScript(["query", "query"], driver), true, driver);
  }
});

test("a T-SQL or MySQL batch keeps its meaning", () => {
  for (const driver of ["sqlserver", "mysql", "odbc", "mongo"]) {
    assert.equal(selectionAsScript(["other", "query"], driver), false, driver);
  }
});

test("client commands always run as a script", () => {
  assert.equal(selectionAsScript(["command", "query"], "sqlserver"), true);
  assert.equal(selectionAsScript(["command"], "postgres"), true);
});
