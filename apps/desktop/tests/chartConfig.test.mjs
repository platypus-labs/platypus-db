// Unit tests of the chart's saved configuration and first choice (src/analysis/chartConfig.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { autoConfig, chartOf, savedCharts, toRequest, validConfig, withChart } from "../src/analysis/chartConfig.ts";

const col = (name, numeric, db_type = numeric ? "integer" : "text") => ({ name, numeric, db_type });

test("bars over the first text column, the first number columns up", () => {
  const columns = [col("id", true), col("region", false), col("a", true), col("b", true), col("c", true)];
  assert.deepEqual(autoConfig(columns), { kind: "bar", x: "region", y: ["id", "a", "b"], aggregate: "sum" });
});

test("lines over a date or a number", () => {
  assert.equal(autoConfig([col("day", false, "date"), col("n", true)]).kind, "line");
  assert.deepEqual(autoConfig([col("x", true), col("y", true)]), { kind: "line", x: "x", y: ["y"], aggregate: "sum" });
  assert.deepEqual(autoConfig([col("s", false)]), { kind: "bar", x: "s", y: [], aggregate: "count" });
});

test("a chart naming a column that is gone is chosen again", () => {
  const columns = [col("region", false), col("total", true)];
  const config = { kind: "pie", x: "region", y: ["total"], aggregate: "sum" };
  assert.equal(validConfig(config, columns), config);
  assert.equal(validConfig({ ...config, y: ["amount"] }, columns), null);
  assert.equal(validConfig({ ...config, x: "city" }, columns), null);
});

test("requests name columns by position", () => {
  const columns = [col("region", false), col("total", true), col("n", true)];
  assert.deepEqual(toRequest({ kind: "bar", x: "region", y: ["n", "total"], aggregate: "avg" }, columns), {
    kind: "bar",
    x: 0,
    y: [2, 1],
    aggregate: "avg",
  });
  assert.equal(toRequest({ kind: "bar", x: "gone", y: [], aggregate: "sum" }, columns), null);
});

test("a statement keeps its chart, whatever the spacing and final semicolon", () => {
  const config = { kind: "bar", x: "r", y: ["t"], aggregate: "sum" };
  const charts = withChart([], " SELECT 1; ", { shown: true });
  assert.deepEqual(chartOf(charts, "SELECT 1"), { query: "SELECT 1", shown: true, config: null });
  const changed = withChart(charts, "SELECT 1", { config });
  assert.equal(changed.length, 1);
  assert.deepEqual(chartOf(changed, "SELECT 1;"), { query: "SELECT 1", shown: true, config });
  assert.equal(chartOf(changed, "SELECT 2"), null);
});

test("saved charts drop what an older or damaged save holds", () => {
  const good = { query: "q", shown: true, config: { kind: "line", x: "a", y: ["b"], aggregate: "max" } };
  const bad = { query: "r", shown: "yes", config: { kind: "radar", x: "a", y: ["b"], aggregate: "sum" } };
  assert.deepEqual(savedCharts([good, bad, null, 3]), [good, { query: "r", shown: false, config: null }]);
  assert.deepEqual(savedCharts("nope"), []);
});
