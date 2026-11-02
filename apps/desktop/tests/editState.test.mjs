// Unit tests of the data editor's pending changes (src/grid/edit/state.ts): marking many
// rows for deletion is one state change, and the request lists every row once.
import assert from "node:assert/strict";
import { test } from "node:test";
import { EMPTY, addRows, changedRows, deleteRows, kindsChanged, setCells, toChanges } from "../src/grid/edit/state.ts";

const target = { table: { schema: "public", name: "study_type" }, key: [0], unique_key: true, table_columns: [0, 1], defaults: [null, null], generated: [null, null] };
const range = (from, n) => Array.from({ length: n }, (_, i) => from + i);

test("kindsChanged counts the kinds of pending change, not the rows", () => {
  assert.equal(kindsChanged(EMPTY), 0);
  let s = addRows(EMPTY, [new Map([[1, { kind: "value", text: "new" }]])]);
  assert.equal(kindsChanged(s), 1);
  s = setCells(s, [{ gridRow: 1, col: 1, value: { kind: "value", text: "x" } }]);
  assert.equal(kindsChanged(s), 2);
  s = deleteRows(s, [3]);
  assert.equal(kindsChanged(s), 3);
});

test("deleting rows drops their changes and new rows, once each", () => {
  let s = addRows(EMPTY, [new Map([[1, { kind: "value", text: "new" }]])]);
  s = setCells(s, [{ gridRow: 2, col: 1, value: { kind: "value", text: "x" } }]);
  s = deleteRows(s, [0, 2, 3, 3]);
  assert.equal(s.inserts.length, 0);
  assert.equal(s.updates.size, 0);
  assert.deepEqual([...s.deletes], [1, 2]);
  assert.deepEqual(toChanges(s, target), { ...target, updates: [], inserts: [], deletes: [1, 2] });
});

for (const n of [100, 1000, 100_000]) {
  test(`marking ${n} rows for deletion is one quick state change`, () => {
    const started = performance.now();
    const s = deleteRows(EMPTY, range(0, n));
    const changes = toChanges(s, target);
    const ms = performance.now() - started;
    assert.equal(changedRows(s), n);
    assert.equal(changes.deletes.length, n);
    // Linear work: well under a frame even for a whole result.
    assert.ok(ms < (n > 1000 ? 500 : 50), `${n} rows: ${ms.toFixed(1)} ms`);
  });
}
