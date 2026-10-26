// Unit tests of the tables "Query with SQL" makes of a tab's results (src/analysis/resultHosts.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { initialSql, resultTables, resultsWithRows } from "../src/analysis/resultHosts.ts";

const rows = (jobId) => ({ jobId, status: { kind: "rows" } });

test("results with rows keep their place among the results", () => {
  const results = [rows(4), { jobId: null, status: { kind: "error" } }, rows(9), { jobId: 5, status: { kind: "affected" } }];
  assert.deepEqual(resultsWithRows(results), [
    { jobId: 4, position: 1 },
    { jobId: 9, position: 3 },
  ]);
});

test("one result is the table result", () => {
  const tables = resultTables([{ jobId: 4, position: 1 }], 4);
  assert.deepEqual(tables, [{ job_id: 4, table: "result" }]);
  assert.equal(initialSql(tables), "SELECT * FROM result");
});

test("several results are also r1, r2, … by their place, the chosen one is result", () => {
  const tables = resultTables(
    [
      { jobId: 4, position: 1 },
      { jobId: 9, position: 3 },
    ],
    9,
  );
  assert.deepEqual(tables, [
    { job_id: 9, table: "result" },
    { job_id: 4, table: "r1" },
    { job_id: 9, table: "r3" },
  ]);
  assert.equal(initialSql(tables), "-- result, r1, r3\nSELECT * FROM result");
});

test("without a chosen result the last is result; no result makes no table", () => {
  const two = [
    { jobId: 4, position: 1 },
    { jobId: 9, position: 2 },
  ];
  assert.deepEqual(resultTables(two, null)[0], { job_id: 9, table: "result" });
  assert.deepEqual(resultTables([], null), []);
});
