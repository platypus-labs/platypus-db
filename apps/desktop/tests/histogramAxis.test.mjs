// Unit tests of the histogram axis helpers (src/analysis/histogramAxis.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { barShare, yMax } from "../src/analysis/histogramAxis.ts";

const bins = [
  { from: "0", to: "5", count: 2 },
  { from: "5", to: "10", count: 8 },
];

test("the y axis tops at the largest bar and never below 1", () => {
  assert.equal(yMax(bins), 8);
  assert.equal(yMax([]), 1);
  assert.equal(yMax([{ from: "a", to: "b", count: 0 }]), 1);
});

test("a bar's height is its share of the axis", () => {
  assert.equal(barShare(bins[0], 8), 0.25);
  assert.equal(barShare(bins[1], 8), 1);
  assert.equal(barShare({ from: "a", to: "b", count: 99 }, 8), 1);
});
