// Unit tests of the automatic layout of the ER diagram (src/erd/layout.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { autoLayout, placeTables } from "../src/erd/layout.ts";

const box = (name, refs = [], height = 50) => ({ name, width: 100, height, refs });

test("a table goes right of the tables it references", () => {
  const at = autoLayout([box("items", ["orders", "products"]), box("orders", ["customers"]), box("customers"), box("products")]);
  assert.ok(at.customers.x < at.orders.x);
  assert.ok(at.orders.x < at.items.x);
  assert.equal(at.products.x, at.customers.x);
});

test("tables without foreign keys go in a grid below the others", () => {
  const at = autoLayout([box("a", ["b"]), box("b"), box("x"), box("y"), box("z")]);
  assert.ok(at.x.y > at.b.y + 50);
  assert.equal(at.x.y, at.y.y);
  assert.ok(at.z.y > at.x.y);
});

test("cycles and missing tables do not stop the layout", () => {
  const at = autoLayout([box("a", ["b"]), box("b", ["a"]), box("c", ["gone"])]);
  assert.deepEqual(Object.keys(at).sort(), ["a", "b", "c"]);
  assert.notEqual(at.a.x, at.b.x);
});

test("no two tables of a column overlap", () => {
  const at = autoLayout([box("p"), box("a", ["p"], 80), box("b", ["p"], 80), box("c", ["p"], 80)]);
  const ys = ["a", "b", "c"].map((n) => at[n].y).sort((p, q) => p - q);
  assert.ok(ys[1] >= ys[0] + 80 && ys[2] >= ys[1] + 80);
});

test("placeTables keeps the placed tables and puts the others below them", () => {
  const fixed = { a: { x: 500, y: 300 }, b: { x: 20, y: 20 } };
  const at = placeTables([box("a", ["b"]), box("b"), box("c", ["a"]), box("d")], fixed);
  assert.deepEqual(at.a, fixed.a);
  assert.deepEqual(at.b, fixed.b);
  const bottom = 300 + 50;
  assert.ok(at.c.y >= bottom && at.d.y >= bottom, JSON.stringify(at));
  assert.deepEqual(Object.keys(at).sort(), ["a", "b", "c", "d"]);
});

test("placeTables lays out everything when nothing is placed, and ignores positions of dropped tables", () => {
  const boxes = [box("a", ["b"]), box("b"), box("c")];
  assert.deepEqual(placeTables(boxes, {}), autoLayout(boxes));
  assert.deepEqual(placeTables(boxes, { dropped: { x: 9, y: 9 } }), autoLayout(boxes));
});

test("placeTables has nothing to lay out when all are placed", () => {
  const fixed = { a: { x: 1, y: 2 }, b: { x: 3, y: 4 } };
  assert.deepEqual(placeTables([box("a"), box("b")], fixed), fixed);
});
