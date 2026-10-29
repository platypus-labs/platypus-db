// Unit tests of the schema history tab's choices (src/schema/history.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { NOW, groupChanges, pair, stillThere, toggled } from "../src/schema/history.ts";

test("two states stay picked, the oldest pick going first", () => {
  assert.deepEqual(toggled([], 3), [3]);
  assert.deepEqual(toggled([3], NOW), [3, NOW]);
  assert.deepEqual(toggled([3, NOW], 5), [NOW, 5]);
  assert.deepEqual(toggled([3, 5], 3), [5], "picking again unticks");
});

test("the older state is compared with the newer, Now being the newest", () => {
  assert.equal(pair([]), null);
  assert.equal(pair([4]), null);
  assert.equal(pair([NOW]), null);
  assert.deepEqual(pair([7, 2]), [2, 7]);
  assert.deepEqual(pair([NOW, 2]), [2, NOW]);
  assert.deepEqual(pair([2, NOW]), [2, NOW]);
});

test("changes are grouped by table", () => {
  const c = (table, kind) => ({ table, kind, item: null, from: null, to: null });
  const groups = groupChanges([c("a", "column_added"), c("a", "index_added"), c("b", "table_added")]);
  assert.deepEqual(groups.map((g) => [g.table, g.changes.length]), [["a", 2], ["b", 1]]);
  assert.deepEqual(groupChanges([]), []);
});

test("a deleted snapshot is no longer picked", () => {
  assert.deepEqual(stillThere([1, NOW], [{ id: 2 }]), [NOW]);
  assert.deepEqual(stillThere([2, NOW], [{ id: 2 }]), [2, NOW]);
});
