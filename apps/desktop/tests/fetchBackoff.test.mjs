// Unit tests of the wait before fetching a failed block of rows again (src/grid/fetchBackoff.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { FailedBlocks } from "../src/grid/fetchBackoff.ts";

test("a failed block waits longer after each failure, up to 30 s", () => {
  const f = new FailedBlocks();
  assert.equal(f.ready(3, 0), true);
  assert.equal(f.fail(3, 0), 1000);
  assert.equal(f.ready(3, 999), false, "not refetched at the next redraw");
  assert.equal(f.ready(3, 1000), true);
  assert.equal(f.fail(3, 1000), 2000);
  assert.equal(f.fail(3, 3000), 4000);
  for (let i = 0; i < 10; i++) f.fail(3, 0);
  assert.equal(f.fail(3, 0), 30_000);
});

test("other blocks are not held back, and a block fetched starts afresh", () => {
  const f = new FailedBlocks();
  f.fail(1, 0);
  assert.equal(f.ready(2, 0), true);
  assert.equal(f.any, true);
  f.ok(1);
  assert.equal(f.ready(1, 0), true);
  assert.equal(f.any, false);
  assert.equal(f.fail(1, 0), 1000);
});
