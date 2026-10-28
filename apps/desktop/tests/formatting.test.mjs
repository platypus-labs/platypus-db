// Unit tests of the grid's conditional formatting (src/grid/formatting.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  colorOf,
  complete,
  conditionOf,
  formatCell,
  matches,
  moveRule,
  resolveRules,
  scaleColor,
  parseNumber,
  scaledColumns,
  valid,
} from "../src/grid/formatting.ts";

const plain = parseNumber;
/** A number column's text with the `1.234,5` format. */
const dotted = (text) => parseNumber(text.replaceAll(".", "").replace(",", "."));
const same = (text) => text;
const columns = [{ name: "id", numeric: true }, { name: "status", numeric: false }, { name: "amount", numeric: true }];
const rule = (id, column, condition, extra = {}) => ({ id, column, condition, color: "red", row: false, ...extra });

test("conditions on text, NULL and numbers", () => {
  assert.ok(matches({ kind: "equals", value: "Paid" }, "paid", plain), "text ignores case");
  assert.ok(matches({ kind: "equals", value: "1.5" }, "1.50", plain), "numbers by value");
  assert.ok(!matches({ kind: "equals", value: "x" }, null, plain));
  assert.ok(matches({ kind: "not_equals", value: "x" }, null, plain), "NULL is not x");
  assert.ok(matches({ kind: "contains", value: "AI" }, "failed", plain));
  assert.ok(matches({ kind: "null" }, null, plain));
  assert.ok(!matches({ kind: "null" }, "", plain), "an empty text is not NULL");
  assert.ok(matches({ kind: "not_null" }, "", plain));
  const between = { kind: "between", min: "10", max: "20" };
  assert.ok(matches(between, "10", plain) && matches(between, "20", plain), "bounds included");
  assert.ok(!matches(between, "20.01", plain) && !matches(between, "abc", plain) && !matches(between, null, plain));
  assert.ok(matches({ kind: "between", min: "", max: "0" }, "-3", plain), "an open bound");
  assert.ok(!matches({ kind: "between", min: "", max: "" }, "1", plain), "no bound matches nothing");
  assert.ok(matches({ kind: "between", min: "1000", max: "" }, "1.234,5", dotted), "the grid's number format");
  assert.ok(!matches({ kind: "equals", value: "16" }, "0x10", plain), "decimal numbers only");
  assert.ok(!matches({ kind: "between", min: "10", max: "20" }, "0x10", plain));
  assert.ok(matches({ kind: "scale", to: "green" }, "3", plain));
  assert.ok(!matches({ kind: "scale", to: "green" }, "n/a", plain));
});

test("the first rule that matches colors the cell; row rules color the whole row", () => {
  const rules = resolveRules(
    [
      rule("a", "status", { kind: "equals", value: "late" }, { row: true, color: "orange" }),
      rule("b", "amount", { kind: "between", min: "100", max: "" }),
      rule("c", "missing", { kind: "null" }),
    ],
    columns,
    same,
  );
  assert.equal(rules.length, 2, "a rule on a column the result lacks is left out");
  const row = (values) => (c) => values[c];
  const late = row(["1", "late", "500"]);
  for (const col of [0, 1, 2]) {
    assert.deepEqual(formatCell(rules, col, late, new Map()), { background: colorOf("orange") });
  }
  const big = row(["2", "paid", "500"]);
  assert.equal(formatCell(rules, 0, big, new Map()), undefined);
  assert.deepEqual(formatCell(rules, 2, big, new Map()), { background: colorOf("red") });
  assert.equal(formatCell(rules, 2, row(["3", "paid", undefined]), new Map()), undefined, "not loaded yet");
});

test("a color scale goes from the lowest number to the highest", () => {
  const rules = resolveRules([rule("s", "amount", { kind: "scale", to: "green" })], columns, same);
  assert.deepEqual(scaledColumns(rules), [2]);
  const ranges = new Map([[2, [0, 100]]]);
  const at = (v) => formatCell(rules, 2, (c) => (c === 2 ? v : "x"), ranges)?.background;
  assert.equal(at("0"), scaleColor("red", "green", 0));
  assert.equal(at("100"), scaleColor("red", "green", 1));
  assert.equal(at("50"), scaleColor("red", "green", 0.5));
  assert.equal(at("n/a"), undefined);
  assert.equal(formatCell(rules, 2, () => "5", new Map()), undefined, "no range yet");
  assert.equal(formatCell(rules, 2, () => "5", new Map([[2, [5, 5]]]))?.background, scaleColor("red", "green", 0.5));
  assert.equal(scaleColor("red", "green", 0), colorOf("red"));
});

test("conditions keep their values when the kind changes, and say when they are complete", () => {
  assert.deepEqual(conditionOf("contains", { kind: "equals", value: "x" }), { kind: "contains", value: "x" });
  assert.deepEqual(conditionOf("between"), { kind: "between", min: "", max: "" });
  assert.deepEqual(conditionOf("scale"), { kind: "scale", to: "red" });
  assert.ok(!complete({ kind: "equals", value: " " }));
  assert.ok(complete({ kind: "between", min: "1", max: "" }));
  assert.ok(!complete({ kind: "between", min: "a", max: "" }));
  assert.ok(complete({ kind: "null" }));
});

test("rules move up and down", () => {
  const rules = [rule("a", "id", { kind: "null" }), rule("b", "id", { kind: "null" })];
  assert.deepEqual(moveRule(rules, 1, -1).map((r) => r.id), ["b", "a"]);
  assert.equal(moveRule(rules, 0, -1), rules);
});

test("the grid's number format applies to number columns only", () => {
  const ungroup = (text) => text.replaceAll(".", "").replace(",", ".");
  const cols = [{ name: "version", numeric: false }, { name: "total", numeric: true }];
  const rules = resolveRules(
    [rule("v", "version", { kind: "equals", value: "3.14" }), rule("t", "total", { kind: "between", min: "1000", max: "" })],
    cols,
    ungroup,
  );
  const at = (col, values) => formatCell(rules, col, (c) => values[c], new Map());
  assert.ok(at(0, ["3.14", "1"]), "a text column keeps its dot");
  assert.ok(at(1, ["x", "1.234,5"]), "1.234,5 is 1234.5");
  assert.equal(at(1, ["x", "999,5"]), undefined);
});

test("stored rules are read only when every field is there", () => {
  const ok = rule("a", "id", { kind: "contains", value: "x" });
  assert.ok(valid(ok));
  assert.ok(!valid({ ...ok, condition: { kind: "contains" } }), "no value");
  assert.ok(!valid({ ...ok, id: undefined }));
  assert.ok(!valid({ ...ok, row: "yes" }));
  assert.ok(!valid({ ...ok, condition: { kind: "between", min: "1" } }));
  assert.ok(!valid({ ...ok, condition: { kind: "scale", to: "pink" } }));
  assert.ok(!valid({ ...ok, condition: { kind: "regex", value: "x" } }));
  assert.ok(!valid(null));
});
