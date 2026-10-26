// Unit tests of the data viewer's header sort (src/components/viewerSort.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  clickSort,
  nextSort,
  queryKey,
  savedSorts,
  sortOf,
  statementRange,
  viewerQueries,
  withSort,
} from "../src/components/viewerSort.ts";

const sorted = "SELECT * FROM items WHERE grp = 1\nORDER BY id ASC";

test("a click cycles ascending, descending, none; another column starts ascending", () => {
  assert.deepEqual(nextSort(null, "id"), { column: "id", desc: false });
  assert.deepEqual(nextSort({ column: "id", desc: false }, "id"), { column: "id", desc: true });
  assert.equal(nextSort({ column: "id", desc: true }, "id"), null);
  assert.deepEqual(nextSort({ column: "id", desc: true }, "name"), { column: "name", desc: false });
});

test("the sort holds for the statement it was written into only", () => {
  const sorts = withSort([], `${sorted};`, { column: "id", desc: false, original: null });
  assert.equal(sorts[0].query, sorted);
  assert.deepEqual(sortOf(sorts, ` ${sorted}`), { column: "id", desc: false, original: null });
  // Another statement of the script, or this one edited by hand, starts unsorted.
  assert.equal(sortOf(sorts, "SELECT * FROM items WHERE grp = 2"), null);
  assert.equal(sortOf(sorts, sorted.replace("ASC", "DESC")), null);
  assert.equal(sortOf(undefined, sorted), null);
  // No sort for the statement: it has none of the viewer's.
  assert.deepEqual(withSort(sorts, sorted, null), []);
});

test("each statement of a script keeps its own sort", () => {
  const s1 = "SELECT * FROM items WHERE grp = 1\nORDER BY id DESC";
  const s2 = "SELECT * FROM items WHERE grp = 2\nORDER BY name ASC";
  let sorts = withSort([], s1, { column: "id", desc: true, original: "ORDER BY name" });
  sorts = withSort(sorts, s2, { column: "name", desc: false, original: null });
  assert.deepEqual(sortOf(sorts, s1), { column: "id", desc: true, original: "ORDER BY name" });
  assert.deepEqual(sortOf(sorts, s2), { column: "name", desc: false, original: null });
  // A new sort of a statement replaces its old one; the oldest go past twenty.
  assert.equal(withSort(sorts, s1, { column: "grp", desc: false, original: null }).length, 2);
  for (let i = 0; i < 25; i++) sorts = withSort(sorts, `SELECT ${i}`, { column: "a", desc: false, original: null });
  assert.equal(sorts.length, 20);
  assert.equal(sortOf(sorts, s1), null);
  assert.ok(sortOf(sorts, "SELECT 24"));
});

test("a restored tab reads its sorts, or the one sort saved before", () => {
  const sort = { column: "id", desc: true, query: sorted, original: "ORDER BY name" };
  assert.deepEqual(savedSorts([sort, { column: 1 }, null], undefined), [sort]);
  assert.deepEqual(savedSorts(undefined, { column: "id", desc: false, query: sorted }), [
    { column: "id", desc: false, query: sorted },
  ]);
  // A sort saved before it knew its statement, or none.
  assert.deepEqual(savedSorts(undefined, { column: "id", desc: false }), []);
  assert.deepEqual(savedSorts(undefined, null), []);
});

test("the third click puts back the ORDER BY the statement had before the first", async () => {
  const own = async () => "ORDER BY name DESC";
  const first = await clickSort(null, "id", own);
  assert.deepEqual(first.sort, { column: "id", desc: false, original: "ORDER BY name DESC" });
  // Written into the statement, the sort keeps it: a quick filter, or another column, too.
  const asc = "SELECT * FROM items ORDER BY id ASC";
  const shown = sortOf(withSort([], asc, first.sort), asc);
  const unread = async () => assert.fail("read once, at the first click");
  const second = await clickSort(shown, "id", unread);
  assert.deepEqual(second.sort, { column: "id", desc: true, original: "ORDER BY name DESC" });
  assert.deepEqual(await clickSort(shown, "grp", unread), {
    sort: { column: "grp", desc: false, original: "ORDER BY name DESC" },
    original: "ORDER BY name DESC",
  });
  assert.deepEqual(await clickSort(second.sort, "id", unread), { sort: null, original: "ORDER BY name DESC" });
  // Without one of its own, the third click unsorts.
  const none = await clickSort(null, "id", async () => null);
  assert.deepEqual(await clickSort({ ...none.sort, desc: true }, "id", unread), { sort: null, original: null });
  // Edited by hand, the statement starts over: its ORDER BY then is the one to put back.
  assert.equal(sortOf(withSort([], asc, first.sort), asc.replace("id ASC", "grp ASC")), null);
  // A sort saved before the original was kept puts none back.
  assert.equal(sortOf([{ column: "id", desc: false, query: asc }], asc).original, null);
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
