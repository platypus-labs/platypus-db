// Unit tests of the saved results' panel (src/analysis/savedResultsModel.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { defaultName, filterSaved } from "../src/analysis/savedResultsModel.ts";

test("the default name is the first line of the statement", () => {
  assert.equal(defaultName("SELECT * FROM t;\nWHERE x", "Result"), "SELECT * FROM t");
  assert.equal(defaultName("\n\n  SELECT   a,\n b FROM t", "Result"), "SELECT a,");
});

test("comments and blank lines are skipped, and an empty statement gets the fallback", () => {
  assert.equal(defaultName("-- the daily totals\nSELECT 1", "Result"), "SELECT 1");
  assert.equal(defaultName("-- only a comment", "Result"), "Result");
  assert.equal(defaultName("  \n ", "Result"), "Result");
});

test("a long first line is cut with an ellipsis", () => {
  const name = defaultName(`SELECT ${"column_name, ".repeat(20)} FROM t`, "Result");
  assert.ok([...name].length <= 60);
  assert.ok(name.endsWith("…"));
  assert.ok(!name.endsWith(" …"));
  // Characters outside the BMP are not split.
  assert.equal([...defaultName("😀".repeat(80), "Result")].length, 60);
});

test("a search needs every word, in the name, the query or the connection", () => {
  const all = [
    { name: "Daily totals", sql: "SELECT day, sum(n) FROM sales", connection: "Shop" },
    { name: "Users", sql: "SELECT * FROM users", connection: "Blog" },
  ];
  assert.deepEqual(filterSaved(all, ""), all);
  assert.deepEqual(filterSaved(all, "  "), all);
  assert.deepEqual(filterSaved(all, "TOTALS shop"), [all[0]]);
  assert.deepEqual(filterSaved(all, "from users"), [all[1]]);
  assert.deepEqual(filterSaved(all, "totals blog"), []);
});
