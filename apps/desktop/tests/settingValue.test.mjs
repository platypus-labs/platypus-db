// Unit tests of numbers typed into the Settings (src/components/settingValue.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { numberRule, settingNumber } from "../src/components/settingValue.ts";

test("a whole step takes whole numbers only", () => {
  assert.equal(settingNumber("2.5", { min: 1, max: 8, step: 1 }), null);
  assert.equal(settingNumber("4", { min: 1, max: 8, step: 1 }), 4);
  assert.equal(settingNumber("10.5", { min: 1, step: 100 }), null);
  assert.equal(settingNumber("150", { min: 1, step: 100 }), 150, "not only multiples of the step");
});

test("a fractional step takes fractions", () => {
  assert.equal(settingNumber("2.5", { min: 0, max: 86400, step: 0.5 }), 2.5);
});

test("bounds, blanks and words are refused", () => {
  assert.equal(settingNumber("0", { min: 1 }), null);
  assert.equal(settingNumber("9", { max: 8 }), null);
  assert.equal(settingNumber("  ", {}), null);
  assert.equal(settingNumber("abc", {}), null);
  assert.equal(settingNumber("Infinity", {}), null);
});

test("a rejected number names the bounds the setting has", () => {
  assert.equal(numberRule({ min: 1, max: 8 }), "between");
  assert.equal(numberRule({ min: 1 }), "min");
  assert.equal(numberRule({ max: 8 }), "max");
  assert.equal(numberRule({}), "number");
});
