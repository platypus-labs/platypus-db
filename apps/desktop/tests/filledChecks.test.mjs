// Unit tests of what a statement asks for once its `${name}` values are filled in
// (src/components/filledChecks.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { asksMore } from "../src/components/filledChecks.ts";

const statement = (kind, unfiltered = []) => ({ sql: "", span: { kind, unfiltered } });
const every = { schema: null, name: "orders", verb: "DELETE", limited: false };

test("a value that drops the WHERE asks again", () => {
  // DELETE FROM orders WHERE id = ${id}, with id = "id OR 1=1" still has a WHERE; with the
  // whole condition in a variable it has none.
  assert.equal(asksMore([statement("dml")], [statement("dml", [every])]), true);
  assert.equal(asksMore([statement("dml", [every])], [statement("dml", [every])]), false);
});

test("a value that turns a read into a change asks again", () => {
  assert.equal(asksMore([statement("query")], [statement("query"), statement("dml")]), true);
  assert.equal(asksMore([statement("query")], [statement("ddl")]), true);
  assert.equal(asksMore([statement("query")], [statement("query")]), false);
  assert.equal(asksMore([statement("dml")], [statement("dml")]), false);
});
