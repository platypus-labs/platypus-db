// Unit tests of totals shown from their exact text (src/grid/exactNumber.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { formatExact } from "../src/grid/exactNumber.ts";

const format = new Intl.NumberFormat("en-US", { maximumFractionDigits: 6 });

test("big integers keep every digit", () => {
  assert.equal(formatExact(format, 9007199254740992, "9007199254740993"), "9,007,199,254,740,993");
  assert.equal(formatExact(format, -9007199254740992, "-9007199254740993"), "-9,007,199,254,740,993");
});

test("decimals format from their text", () => {
  assert.equal(formatExact(format, 1.1, "1.10"), "1.1");
  assert.equal(formatExact(format, 12345678901234567, "12345678901234567.25"), "12,345,678,901,234,567.25");
});

test("without exact text the number is used", () => {
  assert.equal(formatExact(format, 0.5, null), "0.5");
  assert.equal(formatExact(format, 3, undefined), "3");
});
