// Unit tests of the rows a long navigator tree renders (src/navigator/rowWindow.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { ROW_HEIGHT, rowWindow, scrollToRow, stepAt } from "../src/navigator/rowWindow.ts";

test("a tree of a few hundred rows is rendered whole", () => {
  assert.deepEqual(rowWindow(0, 0), { from: 0, to: 0 });
  assert.deepEqual(rowWindow(0, 250), { from: 0, to: 250 });
});

test("a long tree renders the rows around the view, with a margin past either edge", () => {
  const count = 10_003;
  for (const scrollTop of [0, 1, 2799, 2800, 140_000, 279_000]) {
    const { from, to } = rowWindow(stepAt(scrollTop), count);
    const first = Math.floor(scrollTop / ROW_HEIGHT);
    // The tallest view shows 100 rows.
    const last = Math.min(count, first + 100);
    assert.ok(from <= Math.max(0, first - 100), `${scrollTop}: from ${from}, first ${first}`);
    assert.ok(to >= Math.min(count, last + 100), `${scrollTop}: to ${to}, last ${last}`);
    assert.ok(to - from <= 500, `${scrollTop}: ${to - from} rows`);
  }
});

test("scrolling within a step keeps the same window", () => {
  assert.equal(stepAt(0), stepAt(100 * ROW_HEIGHT - 1));
  assert.notEqual(stepAt(0), stepAt(100 * ROW_HEIGHT));
});

test("a tree that shrank below the window renders its rows", () => {
  // Scrolled far down, then filtered to 13 rows before the scroll event comes.
  assert.deepEqual(rowWindow(50, 13), { from: 0, to: 13 });
  assert.deepEqual(rowWindow(50, 4_000), { from: 3_800, to: 4_000 });
});

test("a row out of view is scrolled to, the least", () => {
  const height = 10 * ROW_HEIGHT;
  // Above: at the top of the view.
  assert.equal(scrollToRow(5, 50 * ROW_HEIGHT, height), 5 * ROW_HEIGHT);
  // Below: at the bottom of the view.
  assert.equal(scrollToRow(100, 0, height), 91 * ROW_HEIGHT);
  // Already in view: the scroll stays.
  assert.equal(scrollToRow(3, 0, height), 0);
});
