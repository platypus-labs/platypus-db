// Unit tests of the navigator's type-ahead (src/navigator/typeAhead.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { isTypeAheadKey, nextTyped, typeAheadIndex } from "../src/navigator/typeAhead.ts";

const k = (key, mods = {}) => ({ key, ctrlKey: false, altKey: false, metaKey: false, ...mods });

test("only plain printable keys", () => {
  assert.equal(isTypeAheadKey(k("a")), true);
  assert.equal(isTypeAheadKey(k("Enter")), false);
  assert.equal(isTypeAheadKey(k("F2")), false);
  assert.equal(isTypeAheadKey(k("a", { ctrlKey: true })), false);
  assert.equal(isTypeAheadKey(k("a", { metaKey: true })), false);
});

test("typed text accumulates and resets after a pause", () => {
  assert.equal(nextTyped("or", "d", 200), "ord");
  assert.equal(nextTyped("or", "d", 1500), "d");
});

test("finds the next row forward, wrapping, case-insensitive", () => {
  const labels = ["Alpha", "orders", "Orders2", "users"];
  assert.equal(typeAheadIndex(labels, 0, "o"), 1);
  assert.equal(typeAheadIndex(labels, 1, "o"), 2);
  assert.equal(typeAheadIndex(labels, 2, "o"), 1);
  assert.equal(typeAheadIndex(labels, 3, "AL"), 0);
  // A longer prefix stays on the row that still matches it.
  assert.equal(typeAheadIndex(["zebra", "zero"], 0, "ze"), 0);
  assert.equal(typeAheadIndex(["zebra", "zero"], 0, "zer"), 1);
  assert.equal(typeAheadIndex(labels, 0, "zz"), -1);
});

test("a repeated letter cycles; the current row can be the only match", () => {
  const labels = ["a1", "b", "a2"];
  assert.equal(typeAheadIndex(labels, 0, "aa"), 2);
  assert.equal(typeAheadIndex(["only"], 0, "o"), 0);
  assert.equal(typeAheadIndex([], 0, "o"), -1);
});
