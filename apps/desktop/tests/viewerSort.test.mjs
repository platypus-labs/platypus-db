// Unit tests of the data viewer's header sort (src/components/viewerSort.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { nextSort, queryKey, sortIn, sortOf, statementRange, viewerQueries } from "../src/components/viewerSort.ts";

const sorted = "SELECT * FROM items WHERE grp = 1\nORDER BY id ASC";

test("a click cycles ascending, descending, none; another column starts ascending", () => {
  assert.deepEqual(nextSort(null, "id"), { column: "id", desc: false });
  assert.deepEqual(nextSort({ column: "id", desc: false }, "id"), { column: "id", desc: true });
  assert.equal(nextSort({ column: "id", desc: true }, "id"), null);
  assert.deepEqual(nextSort({ column: "id", desc: true }, "name"), { column: "name", desc: false });
});

test("the sort holds for the statement it was written into only", () => {
  const sort = sortIn({ column: "id", desc: false }, `${sorted};`);
  assert.equal(sort.query, sorted);
  assert.deepEqual(sortOf(sort, ` ${sorted}`), { column: "id", desc: false });
  // Another statement of the script, or this one edited by hand, starts unsorted.
  assert.equal(sortOf(sort, "SELECT * FROM items WHERE grp = 2"), null);
  assert.equal(sortOf(sort, sorted.replace("ASC", "DESC")), null);
  // A sort saved before it knew its statement, or none.
  assert.equal(sortOf({ column: "id", desc: false }, sorted), null);
  assert.equal(sortOf(null, sorted), null);
  assert.equal(sortIn(null, sorted), null);
});

test("the statement's range in the editor, while the editor holds it", () => {
  const text = `SELECT 1;\n${sorted}`;
  const offset = text.indexOf("SELECT *");
  assert.deepEqual(statementRange({ sql: sorted, offset }, text), { from: offset, to: text.length });
  assert.equal(statementRange({ sql: sorted, offset }, text.replace("grp = 1", "grp = 10")), null);
  assert.equal(statementRange({ sql: sorted, offset: -1 }, text), null);
  assert.equal(queryKey("  SELECT 1;\n"), "SELECT 1");
});

test("only the viewer's own queries and their rewrites read the viewer's table", () => {
  const own = viewerQueries("SELECT * FROM items WHERE grp = 1", "SELECT * FROM items", null);
  assert.equal(own.has("SELECT * FROM items;"), true);
  assert.equal(own.has(" SELECT * FROM items WHERE grp = 1"), true);
  // The header sort and the quick filters of an own query give own queries, and so on.
  assert.equal(own.rewritten("SELECT * FROM items", "SELECT * FROM items\nORDER BY id ASC"), true);
  assert.equal(own.has("SELECT * FROM items\nORDER BY id ASC"), true);
  assert.equal(own.rewritten("SELECT * FROM items\nORDER BY id ASC", "SELECT * FROM items WHERE id = 2\nORDER BY id ASC"), true);
  assert.equal(own.has("SELECT * FROM items WHERE id = 2\nORDER BY id ASC"), true);
  // Another table's query typed in the tab is not one, nor is its sort.
  const other = "SELECT * FROM customers";
  assert.equal(own.has(other), false);
  assert.equal(own.rewritten(other, `${other}\nORDER BY name ASC`), false);
  assert.equal(own.has(`${other}\nORDER BY name ASC`), false);
  // A restored tab that saved no query of its own knows none but the given ones.
  assert.equal(viewerQueries(null, undefined).has(""), false);
});
