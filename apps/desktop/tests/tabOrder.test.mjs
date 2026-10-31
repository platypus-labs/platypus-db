// Unit tests of moving a tab in the bar (src/shell/tabOrder.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { moveTab, neighbour } from "../src/shell/tabOrder.ts";

const tab = (key, pinned) => ({ key, pinned });
const keys = (tabs) => tabs.map((t) => t.key);

test("a tab dropped before another goes before it", () => {
  assert.deepEqual(keys(moveTab([tab(1), tab(2), tab(3)], 3, 1, false)), [3, 1, 2]);
});

test("a tab dropped after another goes behind it", () => {
  assert.deepEqual(keys(moveTab([tab(1), tab(2), tab(3)], 1, 2, true)), [2, 1, 3]);
  assert.deepEqual(keys(moveTab([tab(1), tab(2), tab(3)], 1, 3, true)), [2, 3, 1]);
});

test("a drop that changes nothing returns the same array", () => {
  const tabs = [tab(1), tab(2), tab(3)];
  assert.equal(moveTab(tabs, 2, 2, false), tabs);
  assert.equal(moveTab(tabs, 2, 1, true), tabs);
  assert.equal(moveTab(tabs, 2, 3, false), tabs);
  assert.equal(moveTab(tabs, 9, 1, false), tabs);
});

test("a pinned tab stays among the pinned ones", () => {
  const tabs = [tab(1, true), tab(2, true), tab(3)];
  assert.equal(moveTab(tabs, 3, 1, false), tabs);
  assert.equal(moveTab(tabs, 1, 3, true), tabs);
  assert.deepEqual(keys(moveTab(tabs, 2, 1, false)), [2, 1, 3]);
});

test("the neighbour is the next tab in the bar, none past an end", () => {
  const tabs = [tab(1), tab(2), tab(3)];
  assert.equal(neighbour(tabs, 2, -1).key, 1);
  assert.equal(neighbour(tabs, 2, 1).key, 3);
  assert.equal(neighbour(tabs, 1, -1), null);
  assert.equal(neighbour(tabs, 9, 1), null);
});
