// Unit tests of the pivot of a result (src/analysis/pivot.ts): the aggregates a column allows,
// the settings the form's rules leave, and the DuckDB query made from them.
import assert from "node:assert/strict";
import { test } from "node:test";
import { aggregatesFor, normalize, pivotCountSql, pivotSql, quote } from "../src/analysis/pivot.ts";

const COLUMNS = [
  { name: "region", numeric: false },
  { name: "month", numeric: false },
  { name: "amount", numeric: true },
  { name: "we\"ird", numeric: true },
];
const BASE = { rows: ["region"], column: null, value: null, aggregate: "count", totals: false, sort: "keys" };
const sql = (patch) => pivotSql({ ...BASE, ...patch }, "Total");

test("identifiers are quoted with their quotes doubled", () => {
  assert.equal(quote("region"), '"region"');
  assert.equal(quote('we"ird'), '"we""ird"');
  assert.equal(quote("two words"), '"two words"');
});

test("only a number column takes sum and average", () => {
  assert.deepEqual(aggregatesFor(null), ["count"]);
  assert.deepEqual(aggregatesFor({ name: "amount", numeric: true }), ["count", "count_distinct", "sum", "avg", "min", "max"]);
  assert.deepEqual(aggregatesFor({ name: "region", numeric: false }), ["count", "count_distinct", "min", "max"]);
});

test("a plain grouping counts rows or aggregates the value", () => {
  assert.equal(sql({}), 'SELECT "region", count(*) AS "count"\nFROM result\nGROUP BY "region"\nORDER BY "region"');
  assert.equal(
    sql({ value: "amount", aggregate: "sum", sort: "value_desc" }),
    'SELECT "region", sum("amount") AS "sum_amount"\nFROM result\nGROUP BY "region"\nORDER BY "sum_amount" DESC NULLS LAST',
  );
  assert.match(sql({ rows: ["region", "month"], sort: "keys_desc" }), /GROUP BY "region", "month"\nORDER BY "region" DESC, "month" DESC$/);
  assert.match(sql({ value: "amount", aggregate: "count_distinct" }), /count\(DISTINCT "amount"\) AS "count_distinct_amount"/);
  assert.match(sql({ value: "amount", aggregate: "count" }), /count\("amount"\) AS "count_amount"/);
});

test("the aggregate's name does not clash with a row field", () => {
  assert.match(sql({ rows: ["count"] }), /count\(\*\) AS "count_"/);
});

test("totals add a row computed over all the rows, last whatever the sort", () => {
  const query = sql({ value: "amount", aggregate: "avg", totals: true, sort: "value_asc" });
  assert.match(query, /^WITH src AS \(SELECT CAST\("region" AS VARCHAR\) AS "__k1", "region" AS "__s1", "amount" AS "__v" FROM result\),/);
  assert.match(query, /rolled AS \(SELECT \* FROM src UNION ALL SELECT '__total__', NULL, "__v" FROM src\)/);
  assert.match(query, /THEN 'Total' ELSE "__k1" END AS "region", avg\("__v"\) AS "avg_amount"/);
  assert.match(query, /GROUP BY "__k1", "__s1"\nORDER BY "__k1" IS NOT DISTINCT FROM '__total__', "avg_amount" ASC NULLS LAST$/);
});

test("with several row fields the totals label only the first", () => {
  const query = sql({ rows: ["region", "month"], totals: true });
  assert.match(query, /CASE WHEN "__k1" = '__total__' THEN 'Total' ELSE "__k1" END AS "region", CASE WHEN "__k2" = '__total__' THEN NULL ELSE "__k2" END AS "month"/);
  assert.match(query, /SELECT '__total__', '__total__', NULL, NULL FROM src\)/);
});

test("a column field makes a PIVOT, with NULL as a column of its own", () => {
  assert.equal(
    sql({ column: "month", value: "amount", aggregate: "sum" }),
    [
      `PIVOT (SELECT "region", CASE WHEN "month" IS NULL THEN 'NULL' WHEN regexp_matches(CAST("month" AS VARCHAR), '^''*NULL$') THEN '''' || CAST("month" AS VARCHAR) ELSE CAST("month" AS VARCHAR) END AS "month", "amount" AS "__v" FROM result)`,
      'ON "month" USING sum("__v")',
      'GROUP BY "region"',
      'ORDER BY "region"',
    ].join("\n"),
  );
  assert.match(sql({ column: "month" }), /ON "month" USING count\(\*\)/);
  assert.doesNotMatch(sql({ column: "month" }), /__v/);
});

test("pivot totals add a column and a row, each aggregated over its rows", () => {
  const query = sql({ column: "month", value: "amount", aggregate: "avg", totals: true, sort: "value_desc" });
  assert.match(query, /UNION ALL SELECT "__k1", "__s1", '__total__', "__v" FROM src/);
  assert.match(query, /UNION ALL SELECT '__total__', NULL, "__c", "__v" FROM src/);
  assert.match(query, /UNION ALL SELECT '__total__', NULL, '__total__', "__v" FROM src/);
  assert.match(query, /\* EXCLUDE \("__k1", "__s1", "__total__"\), "__total__" AS "Total"/);
  assert.match(query, /FROM \(PIVOT rolled ON "__c" USING avg\("__v"\) GROUP BY "__k1", "__s1"\)/);
  assert.match(query, /ORDER BY "__k1" IS NOT DISTINCT FROM '__total__', "__total__" DESC NULLS LAST$/);
});

test("with totals the rows sort by the field's own type, not by its text", () => {
  const query = sql({ rows: ["region", "month"], totals: true, sort: "keys_desc" });
  assert.match(query, /"region" AS "__s1", "month" AS "__s2"/);
  assert.match(query, /ORDER BY "__k1" IS NOT DISTINCT FROM '__total__', "__s1" DESC, "__s2" DESC$/);
  assert.match(sql({ column: "month", totals: true }), /ORDER BY "__k1" IS NOT DISTINCT FROM '__total__', "__s1"$/);
});

test("identifiers and the total's label are quoted in the query", () => {
  const query = pivotSql({ ...BASE, rows: ['we"ird'], column: "month", totals: true }, "It's all");
  assert.match(query, /CAST\("we""ird" AS VARCHAR\) AS "__k1"/);
  assert.match(query, /THEN 'It''s all' ELSE/);
  assert.match(query, /"__total__" AS "It's all"/);
});

test("the value may also be a row field", () => {
  assert.match(sql({ rows: ["amount"], column: "month", value: "amount", aggregate: "max" }), /"amount", CASE WHEN .*"amount" AS "__v"/);
});

test("the pivot columns are counted as the PIVOT makes them", () => {
  assert.equal(pivotCountSql("month"), `SELECT count(DISTINCT CASE WHEN "month" IS NULL THEN 'NULL' WHEN regexp_matches(CAST("month" AS VARCHAR), '^''*NULL$') THEN '''' || CAST("month" AS VARCHAR) ELSE CAST("month" AS VARCHAR) END) FROM result`);
});

test("the settings keep to the form's rules", () => {
  const ok = normalize({ ...BASE, column: "month", value: "amount", aggregate: "sum" }, COLUMNS);
  assert.deepEqual(ok, { ...BASE, column: "month", value: "amount", aggregate: "sum" });
  // The row fields need one column at least; one missing from the result goes.
  assert.equal(normalize({ ...BASE, rows: [] }, COLUMNS), null);
  assert.equal(normalize({ ...BASE, rows: ["gone"] }, COLUMNS), null);
  assert.deepEqual(normalize({ ...BASE, rows: ["region", "gone", "region"] }, COLUMNS)?.rows, ["region"]);
  // A row field cannot be the column field too.
  assert.equal(normalize({ ...BASE, column: "region" }, COLUMNS)?.column, null);
  // Sum of text is refused: back to counting.
  assert.equal(normalize({ ...BASE, value: "month", aggregate: "sum" }, COLUMNS)?.aggregate, "count");
  assert.equal(normalize({ ...BASE, value: null, aggregate: "max" }, COLUMNS)?.aggregate, "count");
  assert.equal(normalize({ ...BASE, value: "gone", aggregate: "max" }, COLUMNS)?.value, null);
  // A sort by value needs a column to sort by: no column field, or totals.
  assert.equal(normalize({ ...BASE, column: "month", sort: "value_desc" }, COLUMNS)?.sort, "keys");
  assert.equal(normalize({ ...BASE, column: "month", totals: true, sort: "value_desc" }, COLUMNS)?.sort, "value_desc");
  assert.equal(normalize({ ...BASE, sort: "value_desc" }, COLUMNS)?.sort, "value_desc");
});

test("a NULL and a text spelt NULL do not share a pivot column", () => {
  const query = sql({ column: "month", value: "amount", aggregate: "sum" });
  assert.match(query, /WHEN "month" IS NULL THEN 'NULL' WHEN regexp_matches/);
  assert.match(query, /THEN '''' \|\| CAST\("month" AS VARCHAR\)/);
});
