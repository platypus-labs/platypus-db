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
  scaledColumns,
} from "../src/grid/formatting.ts";

const plain = (text) => text;
/** The grid's text with the `1.234,5` format. */
const dotted = (text) => text.replaceAll(".", "").replace(",", ".");
const columns = [{ name: "id" }, { name: "status" }, { name: "amount" }];
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
  );
  assert.equal(rules.length, 2, "a rule on a column the result lacks is left out");
  const row = (values) => (c) => values[c];
  const late = row(["1", "late", "500"]);
  for (const col of [0, 1, 2]) {
    assert.deepEqual(formatCell(rules, col, late, new Map(), plain), { background: colorOf("orange") });
  }
  const big = row(["2", "paid", "500"]);
  assert.equal(formatCell(rules, 0, big, new Map(), plain), undefined);
  assert.deepEqual(formatCell(rules, 2, big, new Map(), plain), { background: colorOf("red") });
  assert.equal(formatCell(rules, 2, row(["3", "paid", undefined]), new Map(), plain), undefined, "not loaded yet");
});

test("a color scale goes from the lowest number to the highest", () => {
  const rules = resolveRules([rule("s", "amount", { kind: "scale", to: "green" })], columns);
  assert.deepEqual(scaledColumns(rules), [2]);
  const ranges = new Map([[2, [0, 100]]]);
  const at = (v) => formatCell(rules, 2, (c) => (c === 2 ? v : "x"), ranges, plain)?.background;
  assert.equal(at("0"), scaleColor("red", "green", 0));
  assert.equal(at("100"), scaleColor("red", "green", 1));
  assert.equal(at("50"), scaleColor("red", "green", 0.5));
  assert.equal(at("n/a"), undefined);
  assert.equal(formatCell(rules, 2, () => "5", new Map(), plain), undefined, "no range yet");
  assert.equal(formatCell(rules, 2, () => "5", new Map([[2, [5, 5]]]), plain)?.background, scaleColor("red", "green", 0.5));
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
