// Unit tests of running a script on several connections (src/editor/multiRun.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { connectionColumn, mergedSql, planScript, pool, quoteText, tableOf } from "../src/editor/multiRun.ts";

const span = (start, end, extra = {}) => ({ start, end, read_only: true, kind: "query", unfiltered: [], parameters: [], ...extra });

test("the results go under each other, with the connection's name", () => {
  const sql = mergedSql([
    { name: "Sales EU", table: tableOf(0), columns: ["id", "total"] },
    { name: "O'Brien \"US\"", table: tableOf(1), columns: ["id"] },
  ]);
  assert.equal(
    sql,
    [
      "-- One result per connection:",
      "--   c1: Sales EU",
      '--   c2: O\'Brien "US"',
      `SELECT 'Sales EU' AS "connection", * FROM "c1"`,
      "UNION ALL BY NAME",
      `SELECT 'O''Brien "US"' AS "connection", * FROM "c2"`,
    ].join("\n"),
  );
});

test("the connection's column takes a free name", () => {
  assert.equal(connectionColumn([["id"], ["name"]]), "connection");
  assert.equal(connectionColumn([["Connection"], []]), "connection_2");
  assert.equal(connectionColumn([["connection", "connection_2"]]), "connection_3");
  assert.equal(quoteText("it's"), "'it''s'");
});

test("a script's statements, batches and repeats", () => {
  const text = "SELECT 1; UPDATE t SET a = 1;";
  const plan = planScript(text, { statements: [span(0, 8), span(10, 28, { read_only: false, kind: "dml" })], at_cursor: null, batches: [] });
  assert.deepEqual(plan, { statements: ["SELECT 1", "UPDATE t SET a = 1"], writes: true });
  const tsql = "DECLARE @a int\nSELECT @a\nGO 2";
  const batched = planScript(tsql, {
    statements: [span(0, 14, { kind: "other" }), span(15, 24)],
    at_cursor: null,
    batches: [{ first: 0, end: 2, repeat: 2 }],
  });
  assert.deepEqual(batched, { statements: ["DECLARE @a int\nSELECT @a", "DECLARE @a int\nSELECT @a"], writes: false });
});

test("parameters, client commands and nothing at all cannot run", () => {
  const analysis = (statements) => ({ statements, at_cursor: null, batches: [] });
  assert.equal(planScript("", analysis([])), "empty");
  assert.equal(planScript("@set x = 1", analysis([span(0, 10, { kind: "command" })])), "commands");
  assert.equal(
    planScript("SELECT :id", analysis([span(0, 10, { parameters: [{ kind: "named", name: "id", start: 7, end: 10 }] })])),
    "parameters",
  );
});

test("the pool runs a few at a time, in order, and stops when told", async () => {
  let running = 0;
  let most = 0;
  const started = [];
  await pool([1, 2, 3, 4, 5], 2, async (n) => {
    started.push(n);
    running++;
    most = Math.max(most, running);
    await new Promise((r) => setTimeout(r, 5));
    running--;
  }, () => false);
  assert.deepEqual(started, [1, 2, 3, 4, 5]);
  assert.equal(most, 2);
  const some = [];
  await pool([1, 2, 3, 4], 1, async (n) => void some.push(n), () => some.length >= 2);
  assert.deepEqual(some, [1, 2]);
});
