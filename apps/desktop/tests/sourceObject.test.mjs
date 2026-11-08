// Unit tests of the object a query in the editor reads (src/grid/edit/sourceObject.ts): its kind
// comes with it, so that a MongoDB view stays read-only, and MongoDB names are case-sensitive.
import assert from "node:assert/strict";
import { test } from "node:test";
import { sourceObject } from "../src/grid/edit/sourceObject.ts";

const object = (name, kind) => ({ name, kind, comment: null, parent: null });
const objects = [object("orders", "table"), object("order_totals", "view")];

test("a view is found as a view, which the data editor does not edit", () => {
  assert.equal(sourceObject(objects, "order_totals", true)?.kind, "view");
  assert.equal(sourceObject(objects, "orders", true)?.kind, "table");
});

test("MongoDB names are case-sensitive: another case is another collection", () => {
  assert.equal(sourceObject(objects, "Orders", true), undefined);
  assert.equal(sourceObject(objects, "ORDER_TOTALS", true), undefined);
});

test("where names fold, another case finds the object", () => {
  assert.equal(sourceObject(objects, "ORDERS", false)?.name, "orders");
  assert.equal(sourceObject(objects, "Order_Totals", false)?.kind, "view");
  assert.equal(sourceObject(objects, "missing", false), undefined);
});
