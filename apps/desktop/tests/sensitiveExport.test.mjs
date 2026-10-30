// Unit tests of what an export does with the sensitive columns (src/guards/sensitiveExport.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { firstChoice, withSensitive } from "../src/guards/sensitiveExport.ts";

const base = { format: "csv", delimiter: ",", header: true };

test("the sensitive columns are masked, left out or kept as chosen", () => {
  const sensitive = ["email", "phone"];
  assert.deepEqual(withSensitive(base, sensitive, "mask"), { ...base, masked: sensitive, omitted: [] });
  assert.deepEqual(withSensitive(base, sensitive, "omit"), { ...base, masked: [], omitted: sensitive });
  assert.deepEqual(withSensitive({ ...base, masked: ["old"] }, sensitive, "keep"), { ...base, masked: [], omitted: [] });
});

test("a new export masks while masking is on; a saved profile keeps its choice", () => {
  assert.equal(firstChoice(undefined, true), "mask");
  assert.equal(firstChoice(undefined, false), "keep");
  assert.equal(firstChoice({ ...base, omitted: ["email"] }, false), "omit");
  assert.equal(firstChoice({ ...base, masked: ["email"] }, false), "mask");
  assert.equal(firstChoice({ ...base }, true), "keep", "a profile saved keeping the values");
});
