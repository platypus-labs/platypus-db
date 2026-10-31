// Unit tests of the navigator's text filter, a dragged routine's name and the bookmarks of
// dropped objects (src/navigator/filter.ts, src/sqlNames.ts, src/navigator/prefs.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { textFilter } from "../src/navigator/filter.ts";
import { withoutObject } from "../src/navigator/prefs.ts";
import { quoteIdent, routineBase } from "../src/sqlIdent.ts";

test("a bare text matches the objects and the schemas that bear it", () => {
  const f = textFilter("qa4_nav_fa");
  assert.equal(f.wholeSchema("qa4_nav_fa"), true);
  assert.equal(f.wholeSchema("qa4_nav_fb"), false);
  assert.equal(f.matches("qa4_nav_fa", "users"), true);
  assert.equal(f.matches("qa4_nav_fb", "stuff"), false);
  assert.equal(textFilter("use").matches("any", "users"), true);
  assert.equal(textFilter("use").wholeSchema("user_data"), true);
  assert.equal(textFilter("").matches("any", "x"), true);
});

test("schema.object matches the objects of that schema; schema. all of them", () => {
  const f = textFilter("qa4_nav_fa.users");
  assert.equal(f.matches("qa4_nav_fa", "users"), true);
  assert.equal(f.matches("qa4_nav_fa", "orders"), false);
  assert.equal(f.matches("qa4_nav_fb", "users"), false);
  assert.equal(f.wholeSchema("qa4_nav_fa"), false);
  const all = textFilter("qa4_nav_fa.");
  assert.equal(all.wholeSchema("qa4_nav_fa"), true);
  assert.equal(all.matches("qa4_nav_fa", "orders"), true);
  assert.equal(textFilter(".users").matches("s", "users"), true);
});

test("a dragged routine is named without its signature", () => {
  assert.equal(routineBase("f_one(x integer)"), "f_one");
  assert.equal(quoteIdent(routineBase("Mixed Case(a text, b int)"), "postgres"), '"Mixed Case"');
  assert.equal(routineBase("f"), "f");
});

test("dropping an object removes its bookmarks and those of its triggers", () => {
  const mark = (name, kind = "table", parent = null, schema = "main") => ({
    connectionId: "c1", database: null, schema, currentSchema: true, name, kind, parent,
  });
  const marks = [mark("t"), mark("keep"), mark("trg", "trigger", "t"), mark("t", "view"), mark("t", "table", null, "other")];
  const left = withoutObject(marks, { connectionId: "c1", database: null, schema: "main", kind: "table" }, "t");
  assert.deepEqual(left.map((b) => `${b.schema}.${b.kind}.${b.name}`), ["main.table.keep", "main.view.t", "other.table.t"]);
});
