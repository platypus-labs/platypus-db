// Unit tests of the chart's scales, ticks and shapes (src/analysis/chartScale.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  arcPath,
  band,
  categoryLabel,
  categoryTicks,
  extent,
  formatNumber,
  formatTime,
  groupSmallSlices,
  hasMarks,
  linear,
  niceDomain,
  niceTicks,
  pieAngles,
  pieParts,
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
  // Ranges whose step underflows or overflows, or where adding the step loses it, still end.
  assert.deepEqual(niceTicks(0, 5e-324, 5), [0, 5e-324]);
  assert.deepEqual(niceDomain(-Number.MAX_VALUE, Number.MAX_VALUE, 5), [-Number.MAX_VALUE, Number.MAX_VALUE]);
  assert.ok(niceTicks(1e17, 1e17 + 64, 5).length <= 22);
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

test("the last category keeps its label when asked (Other), in place of one too close", () => {
  assert.deepEqual(categoryTicks(10, 3, true), [0, 4, 9]);
  assert.deepEqual(categoryTicks(9, 3, true), [0, 3, 8]);
  assert.deepEqual(categoryTicks(9, 3), [0, 3, 6]);
  assert.deepEqual(categoryTicks(2, 10, true), [0, 1]);
  assert.deepEqual(categoryTicks(0, 10, true), []);
});

test("SQL NULL is told apart from the text NULL", () => {
  assert.equal(categoryLabel(null), "(NULL)");
  assert.equal(categoryLabel("NULL"), "NULL");
});

test("nothing to draw: no category value and no point", () => {
  assert.equal(hasMarks({ series: [] }), false);
  assert.equal(hasMarks({ series: [{ values: [null, null], points: [] }] }), false);
  assert.equal(hasMarks({ series: [{ values: [null, 0], points: [] }] }), true);
  assert.equal(hasMarks({ series: [{ values: [], points: [[0, 1]] }] }), true);
});

test("a pie draws the positive values and says which it leaves out", () => {
  assert.deepEqual(pieParts([5, -3, 2, 0, null]), { drawn: [0, 2], left: [1, 3, 4] });
});

test("numbers follow the language given, not the system's", () => {
  assert.equal(formatNumber(0.8, "it"), "0,8");
  assert.equal(formatNumber(0.8, "en"), "0.8");
});

test("long labels are cut", () => {
  assert.equal(truncate("abcdef", 4), "abc…");
  assert.equal(truncate("abc", 4), "abc");
});

test("numbers under 0.01 keep their significant digits instead of reading 0", () => {
  assert.equal(formatNumber(0.000001, "en-US"), "1E-6");
  assert.equal(formatNumber(0.0000035, "en-US"), "3.5E-6");
  assert.equal(formatNumber(-0.004, "en-US"), "-4E-3");
  assert.equal(formatNumber(0, "en-US"), "0");
  assert.equal(formatNumber(0.01, "en-US"), "0.01");
});

test("a pie gathers the smallest slices into one, keeping the order of the others", () => {
  const values = [5, 1, 9, 2, null, 0, 3];
  const names = ["a", "b", "c", "d", "e", "f", "g"];
  assert.deepEqual(groupSmallSlices(values, names, "Other", false, 3), { values: [5, 9, 3, 3], categories: ["a", "c", "g", "Other"] });
  // Few enough slices: nothing is gathered.
  assert.deepEqual(groupSmallSlices([2, 1], ["x", "y"], "Other", false), { values: [2, 1], categories: ["x", "y"] });
  // The category that already gathers the rest joins the new one.
  assert.deepEqual(groupSmallSlices([4, 3, 2, 7], ["a", "b", "c", "Other"], "Other", true, 2), {
    values: [4, 3, 9],
    categories: ["a", "b", "Other"],
  });
});
