// Unit tests of the chart's scales, ticks and shapes (src/analysis/chartScale.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  arcPath,
  band,
  categoryTicks,
  extent,
  formatNumber,
  formatTime,
  linear,
  niceDomain,
  niceTicks,
  pieAngles,
  timeTicks,
  truncate,
} from "../src/analysis/chartScale.ts";

const DAY = 86_400_000;
const near = (a, b) => assert.ok(Math.abs(a - b) < 1e-9, `${a} ≈ ${b}`);

test("linear maps the domain onto the range, a single value to the middle", () => {
  const y = linear([0, 10], [100, 0]);
  assert.equal(y(0), 100);
  assert.equal(y(5), 50);
  assert.equal(linear([3, 3], [0, 10])(3), 5);
});

test("extent, and a neutral one for no values", () => {
  assert.deepEqual(extent([3, -1, 7]), [-1, 7]);
  assert.deepEqual(extent([]), [0, 1]);
});

test("ticks are round numbers, the domain widened to them", () => {
  assert.deepEqual(niceTicks(0, 100, 5), [0, 20, 40, 60, 80, 100]);
  assert.deepEqual(niceDomain(3, 97, 5), [0, 100]);
  assert.deepEqual(niceDomain(5, 5, 5), [0, 5]);
  assert.deepEqual(niceTicks(4, 4, 5), [4]);
});

test("time ticks fall on whole days over days, and format as dates", () => {
  assert.deepEqual(timeTicks(0, 10 * DAY, 5), [0, 2, 4, 6, 8, 10].map((d) => d * DAY));
  assert.equal(formatTime(0, 10 * DAY), "1970-01-01");
  assert.equal(formatTime(3_600_000, DAY), "1970-01-01 01:00");
});

test("numbers are short", () => {
  assert.equal(formatNumber(1500, "en-US"), "1.5K");
  assert.equal(formatNumber(0.25, "en-US"), "0.25");
});

test("bands and the categories that get a label", () => {
  const b = band(4, 0, 100, 0.2);
  assert.equal(b.step, 25);
  assert.equal(b.width, 20);
  assert.equal(b.at(1), 27.5);
  assert.deepEqual(categoryTicks(10, 3), [0, 4, 8]);
  assert.deepEqual(categoryTicks(2, 10), [0, 1]);
});

test("slices go clockwise from the top; negatives are empty", () => {
  const [a, b, c] = pieAngles([1, 1, -1]);
  near(a[0], -Math.PI / 2);
  near(a[1], Math.PI / 2);
  near(b[1], (3 * Math.PI) / 2);
  near(c[0], c[1]);
  assert.match(arcPath(50, 50, 10, 0, Math.PI / 2), /^M 50 50 L 60\.00 50\.00 A 10 10 0 0 1 50\.00 60\.00 Z$/);
  assert.equal(arcPath(50, 50, 10, -Math.PI / 2, (3 * Math.PI) / 2).match(/A /g).length, 2);
});

test("long labels are cut", () => {
  assert.equal(truncate("abcdef", 4), "abc…");
  assert.equal(truncate("abc", 4), "abc");
});
