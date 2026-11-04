// Unit tests of what an invalid field's message says, in place of the browser's (src/ui/fieldError.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { fieldError } from "../src/ui/fieldError.ts";

test("each kind of invalid value has its message", () => {
  assert.deepEqual(fieldError({ validity: { valueMissing: true } }), { kind: "fill" });
  assert.deepEqual(fieldError({ type: "checkbox", validity: { valueMissing: true } }), { kind: "tick" });
  assert.deepEqual(fieldError({ min: "1", validity: { rangeUnderflow: true } }), { kind: "min", value: "1" });
  assert.deepEqual(fieldError({ max: "65535", validity: { rangeOverflow: true } }), { kind: "max", value: "65535" });
  assert.deepEqual(fieldError({ minLength: 8, validity: { tooShort: true } }), { kind: "short", value: 8 });
  assert.deepEqual(fieldError({ validity: { badInput: true } }), { kind: "number" });
  assert.deepEqual(fieldError({ validity: { typeMismatch: true } }), { kind: "invalid" });
});

test("a message the form set itself is kept", () => {
  assert.deepEqual(fieldError({ validationMessage: "The name is taken.", validity: { customError: true } }), { kind: "own", value: "The name is taken." });
});
