// Unit tests of the admin tab's statistics: the order of the costliest statements, the issues
// of tables and indexes, and which objects show (src/admin/statistics.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { duration } from "../src/admin/sessions.ts";
import { deadShare, issues, queryMatches, shownObjects, sortQueries, tableOf } from "../src/admin/statistics.ts";

const query = (q, total, mean, calls) => ({
  query: q,
  sample: null,
  database: "shop",
  user: "app",
  calls,
  total_seconds: total,
  mean_seconds: mean,
  max_seconds: null,
  rows: null,
});

const table = (name, extra = {}) => ({
  schema: "public",
  name,
  kind: "table",
  table: null,
  size_bytes: 8192,
  rows: 100,
  dead_rows: 0,
  free_bytes: null,
  full_scans: 0,
  index_scans: 0,
  last_vacuum: null,
  last_analyze: "2026-10-29 10:00:00",
  unique: false,
  ...extra,
});

const index = (name, of, extra = {}) => ({
  ...table(name),
  kind: "index",
  table: of,
  rows: null,
  dead_rows: null,
  full_scans: null,
  last_analyze: null,
  ...extra,
});

test("the costliest statements by total time, mean time or calls", () => {
  const all = [query("a", 10, 1, 10), query("b", 5, 5, 1), query("c", 1, 0.01, 100)];
  assert.deepEqual(sortQueries(all, "total").map((q) => q.query), ["a", "b", "c"]);
  assert.deepEqual(sortQueries(all, "mean").map((q) => q.query), ["b", "a", "c"]);
  assert.deepEqual(sortQueries(all, "calls").map((q) => q.query), ["c", "a", "b"]);
  assert.deepEqual(all.map((q) => q.query), ["a", "b", "c"], "the list is left as it was");
});

test("statements filter by text, database and user", () => {
  const q = query("SELECT * FROM Orders", 1, 1, 1);
  assert.ok(queryMatches(q, "orders"));
  assert.ok(queryMatches(q, " SHOP "));
  assert.ok(queryMatches(q, ""));
  assert.ok(!queryMatches(q, "customers"));
});

test("an index no read used is unused, unless it is unique", () => {
  assert.deepEqual(issues(index("i", "t", { index_scans: 0 }), true), ["unused"]);
  assert.deepEqual(issues(index("pk", "t", { index_scans: 0, unique: true }), true), []);
  assert.deepEqual(issues(index("i", "t", { index_scans: 3 }), true), []);
  // A server that does not tell how indexes are used.
  assert.deepEqual(issues(index("i", "t", { index_scans: null }), true), []);
});

test("a big table read whole more than through its indexes", () => {
  assert.deepEqual(issues(table("t", { rows: 50_000, full_scans: 40, index_scans: 2 }), true), ["full_scans"]);
  assert.deepEqual(issues(table("t", { rows: 50_000, full_scans: 2, index_scans: 40 }), true), []);
  assert.deepEqual(issues(table("small", { rows: 500, full_scans: 40 }), true), [], "small tables are read whole at no cost");
});

test("dead rows and unused space", () => {
  const dead = table("t", { rows: 6000, dead_rows: 4000 });
  assert.equal(deadShare(dead), 0.4);
  assert.deepEqual(issues(dead, true), ["dead_rows"]);
  assert.deepEqual(issues(table("t", { rows: 10, dead_rows: 90 }), true), [], "a few dead rows are not worth a vacuum");
  assert.equal(deadShare(table("t", { rows: 0, dead_rows: 0 })), 0);
  const free = table("t", { size_bytes: 100 * 2 ** 20, free_bytes: 40 * 2 ** 20 });
  assert.deepEqual(issues(free, true), ["free_space"]);
  assert.deepEqual(issues(table("t", { size_bytes: 2 ** 20, free_bytes: 2 ** 19 }), true), [], "under 10 MB");
});

test("a table never analyzed, where the server tells", () => {
  const never = table("t", { last_analyze: null, rows: 10 });
  assert.deepEqual(issues(never, true), ["never_analyzed"]);
  assert.deepEqual(issues(never, false), []);
  assert.deepEqual(issues(table("empty", { last_analyze: null, rows: 0 }), true), []);
});

test("only the objects with issues, each table with them", () => {
  const objects = [
    table("orders"),
    index("orders_pkey", "orders", { unique: true }),
    index("orders_note", "orders", { index_scans: 0 }),
    table("customers"),
    index("customers_pkey", "customers", { unique: true, index_scans: 9 }),
  ];
  assert.equal(shownObjects(objects, "", false).length, 5);
  assert.deepEqual(
    shownObjects(objects, "", true).map((o) => o.name),
    ["orders", "orders_note"],
    "the table of an unused index shows above it",
  );
  assert.deepEqual(shownObjects(objects, "cust", false).map((o) => o.name), ["customers", "customers_pkey"]);
  assert.deepEqual(shownObjects(objects, "note", false).map((o) => o.name), ["orders", "orders_note"]);
  assert.equal(tableOf(objects[2]), "orders");
  assert.equal(tableOf(objects[0]), "orders");
});

test("durations under a second in milliseconds", () => {
  assert.equal(duration(0.00042), "0.42 ms");
  assert.equal(duration(0.0125), "12.5 ms");
  assert.equal(duration(3.21), "3.2 s");
  assert.equal(duration(245), "4 min 05 s");
  assert.equal(duration(null), "");
});
