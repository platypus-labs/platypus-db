// Unit tests of the JSON change check (src/grid/edit/jsonChange.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { sameJson } from "../src/grid/edit/jsonChange.ts";

test("formatting is not a change", () => {
  const original = '{"a":1,"b":[true,null],"c":{"d":"x y"}}';
  assert.equal(sameJson(original, JSON.stringify(JSON.parse(original), null, 2)), true);
  assert.equal(sameJson(original, original), true);
});

test("whitespace inside strings counts", () => {
  assert.equal(sameJson('{"a":"x y"}', '{"a":"x  y"}'), false);
  assert.equal(sameJson('{"a":"q\\" "}', '{ "a": "q\\" " }'), true);
});

test("a changed value, key order or number spelling is a change", () => {
  assert.equal(sameJson('{"a":1}', '{"a": 2}'), false);
  assert.equal(sameJson('{"a":1,"b":2}', '{"b":2,"a":1}'), false);
  assert.equal(sameJson("1.0", "1"), false);
});

test("null and invalid JSON are changes", () => {
  assert.equal(sameJson(null, "{}"), false);
  assert.equal(sameJson("{", "{"), false);
  assert.equal(sameJson("{}", "{"), false);
});
