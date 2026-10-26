// Unit tests of what a SQL editor tab runs again without asking (src/components/rerun.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { RunMemory, rerunsUnasked } from "../src/components/rerun.ts";

const select = { sql: "SELECT * FROM orders", readOnly: true };
const del = { sql: "DELETE FROM orders", readOnly: false };

test("only statements that only read run again unasked", () => {
  assert.equal(rerunsUnasked([select]), true);
  assert.equal(rerunsUnasked([select, del]), false);
  assert.equal(rerunsUnasked([del]), false);
  assert.equal(rerunsUnasked([]), false);
});

test("a run declined at its checks leaves the last run as it was", () => {
  const memory = new RunMemory();
  memory.started([select], true);
  memory.ran(1, select);
  // DELETE planned, then declined at the guard: `started` is never called for it.
  assert.deepEqual(memory.last(), [select]);
  assert.equal(memory.readOf(1), select, "saving edits of result 1 runs the SELECT again");
  assert.deepEqual(memory.lastReads(), [select]);
});

test("a statement that changes data never runs again unasked", () => {
  const memory = new RunMemory();
  memory.started([del], true);
  memory.ran(1, del);
  assert.equal(memory.readOf(1), null);
  assert.equal(memory.lastReads(), null);
  assert.deepEqual(memory.last(), [del], "auto-refresh still sees it, to stop");
});

test("each result runs its own statement again; a fresh run forgets the old ones", () => {
  const memory = new RunMemory();
  const other = { sql: "SELECT 2", readOnly: true };
  memory.started([select], true);
  memory.ran(1, select);
  memory.started([other], false);
  memory.ran(2, other);
  assert.equal(memory.readOf(1), select);
  assert.equal(memory.readOf(2), other);
  memory.forget(2);
  assert.equal(memory.readOf(2), null);
  memory.started([other], true);
  assert.equal(memory.readOf(1), null);
});

test("a statement planned but not sent has no result to run again", () => {
  const memory = new RunMemory();
  memory.started([select, select], true);
  memory.ran(1, select);
  assert.equal(memory.readOf(2), null);
});
