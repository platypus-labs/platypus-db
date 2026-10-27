// Unit tests of the catalog moves behind renames (src/catalog/renames.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { renamedObject, tableCatalogMoves } from "../src/catalog/renames.ts";

test("columns move before the table, under the old table name", () => {
  const design = {
    name: "b",
    columns: [
      { name: "x2", original: "x" },
      { name: "y", original: "y" },
      { name: "z", original: null },
    ],
  };
  assert.deepEqual(tableCatalogMoves({ name: "a" }, design), [
    { from: { object: "a", column: "x" }, to: { object: "a", column: "x2" } },
    { from: { object: "a", column: "" }, to: { object: "b", column: "" } },
  ]);
});

test("nothing moves when nothing was renamed", () => {
  assert.deepEqual(tableCatalogMoves({ name: "a" }, { name: "a", columns: [{ name: "x", original: "x" }] }), []);
});

test("a routine keeps its signature", () => {
  assert.equal(renamedObject("f(integer, text)", "g", true), "g(integer, text)");
  assert.equal(renamedObject("f(integer)", "f", true), null);
  assert.equal(renamedObject("t", "u", false), "u");
  assert.equal(renamedObject("t", "", false), null);
});

test("a table's name is renamed whole, parentheses included", () => {
  assert.equal(renamedObject("t(1)", "u", false), "u");
  assert.equal(renamedObject("t(1)", "t(1)", false), null);
});
