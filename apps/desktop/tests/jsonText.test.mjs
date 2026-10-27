// Unit tests of JSON handled as text (src/jsonText.ts): formatting and parsing keep every
// number exactly as written.
import assert from "node:assert/strict";
import { test } from "node:test";
import { JsonNumber, formatJson, isJsonDocument, jsonNumber, parseJson, stringifyJson } from "../src/jsonText.ts";
import { sameJson } from "../src/grid/edit/jsonChange.ts";

const BIG = '{"id": 9007199254740993, "price": 1.10, "e": 1e2, "neg": -0.0, "list": [1E+400, 12345678901234567890]}';

test("formatting keeps numbers as written", () => {
  const out = formatJson(BIG);
  for (const n of ["9007199254740993", "1.10", "1e2", "-0.0", "1E+400", "12345678901234567890"]) {
    assert.ok(out.includes(n), `${n} in ${out}`);
  }
  assert.equal(sameJson(BIG, out), true, "only the layout changed");
});

test("formatting indents objects and arrays, empty ones stay on one line", () => {
  assert.equal(formatJson('{"a":[1,2],"b":{},"c":[ ],"d":{"e":null}}'), '{\n  "a": [\n    1,\n    2\n  ],\n  "b": {},\n  "c": [],\n  "d": {\n    "e": null\n  }\n}');
});

test("formatting leaves strings alone, escapes and structural characters included", () => {
  const text = '{"a":"x, y: {z} [w] \\" \\\\","b" : "  two  spaces "}';
  const out = formatJson(text);
  assert.deepEqual(JSON.parse(out), JSON.parse(text));
  assert.ok(out.includes('"x, y: {z} [w] \\" \\\\"'));
  assert.ok(out.includes('"  two  spaces "'));
});

test("formatting a scalar keeps it", () => {
  assert.equal(formatJson(" 9007199254740993 "), "9007199254740993");
  assert.equal(formatJson('"a"'), '"a"');
});

test("invalid JSON throws a SyntaxError", () => {
  assert.throws(() => formatJson("{"), SyntaxError);
  assert.throws(() => formatJson("{'a':1}"), SyntaxError);
  assert.throws(() => parseJson("[1,]"), SyntaxError);
});

test("only objects and arrays count as JSON documents", () => {
  assert.equal(isJsonDocument("9007199254740993"), false);
  assert.equal(isJsonDocument("0.10"), false);
  assert.equal(isJsonDocument('"text"'), false);
  assert.equal(isJsonDocument("true"), false);
  assert.equal(isJsonDocument("  [1]"), true);
  assert.equal(isJsonDocument('{"a":1}'), true);
  assert.equal(isJsonDocument("{not json}"), false);
});

test("parsing keeps numbers exact, and writing them back too", () => {
  const value = parseJson(BIG);
  assert.ok(value.id instanceof JsonNumber);
  assert.equal(value.id.text, "9007199254740993");
  assert.equal(value.price.text, "1.10");
  assert.equal(value.list[1].text, "12345678901234567890");
  const text = stringifyJson(value);
  assert.equal(sameJson(BIG, text), true, text);
});

test("parsing reads strings, literals and nesting like JSON.parse", () => {
  const text = '{"s":"a\\u00e9\\n","t":true,"f":false,"n":null,"o":{"x":[[],{}]},"__proto__":1}';
  const value = parseJson(text);
  assert.equal(value.s, "aé\n");
  assert.equal(value.t, true);
  assert.equal(value.f, false);
  assert.equal(value.n, null);
  assert.deepEqual(value.o, { x: [[], {}] });
  assert.deepEqual(Object.keys(value), ["s", "t", "f", "n", "o", "__proto__"]);
  assert.equal(Object.getPrototypeOf(value), Object.prototype);
});

test("number spelling check", () => {
  assert.equal(jsonNumber("9007199254740993")?.text, "9007199254740993");
  assert.equal(jsonNumber("-1.5e-3")?.text, "-1.5e-3");
  assert.equal(jsonNumber("NaN"), null);
  assert.equal(jsonNumber("Infinity"), null);
  assert.equal(jsonNumber("01"), null);
  assert.equal(jsonNumber(""), null);
});
