// Unit tests of the data editor's undo and redo (src/grid/edit/history.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { History } from "../src/grid/edit/history.ts";

test("redo takes back the undos, in order", () => {
  const h = new History(10);
  h.record("a");
  h.record("b");
  // Now at "c": undo twice, then redo once.
  assert.equal(h.undo("c"), "b");
  assert.equal(h.undo("b"), "a");
  assert.equal(h.undo("a"), undefined);
  assert.ok(h.canRedo);
  assert.equal(h.redo("a"), "b");
  assert.equal(h.redo("b"), "c");
  assert.equal(h.redo("c"), undefined);
  assert.equal(h.undo("c"), "b");
});

test("a new change drops what was undone", () => {
  const h = new History(10);
  h.record("a");
  assert.equal(h.undo("b"), "a");
  h.record("a");
  assert.ok(!h.canRedo);
  assert.ok(h.canUndo);
});

test("only the last states are kept, and saving clears them", () => {
  const h = new History(2);
  for (const s of ["a", "b", "c"]) h.record(s);
  assert.equal(h.undo("d"), "c");
  assert.equal(h.undo("c"), "b");
  assert.equal(h.undo("b"), undefined);
  h.clear();
  assert.ok(!h.canUndo && !h.canRedo);
});
