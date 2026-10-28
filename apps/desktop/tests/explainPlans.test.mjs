// Unit tests of plan comparison and the graph layout (src/explain/compare.ts, graph.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { alignPlans, change, measureOf, own, total } from "../src/explain/compare.ts";
import { BOX, layoutPlan } from "../src/explain/graph.ts";

const step = (operation, children = [], extra = {}) => ({
  operation,
  detail: null,
  cost: null,
  rows: null,
  actual_rows: null,
  actual_ms: null,
  loops: null,
  self_cost: null,
  self_ms: null,
  full_scan: null,
  properties: [],
  children,
  ...extra,
});

const shape = (pairs) => pairs.map((p) => [p.depth, p.before?.operation ?? null, p.after?.operation ?? null]);

test("the same plan lines up step by step", () => {
  const plan = step("Hash Join", [step("Seq Scan"), step("Hash", [step("Seq Scan")])]);
  assert.deepEqual(shape(alignPlans(plan, plan)), [
    [0, "Hash Join", "Hash Join"],
    [1, "Seq Scan", "Seq Scan"],
    [1, "Hash", "Hash"],
    [2, "Seq Scan", "Seq Scan"],
  ]);
});

test("a replaced step stands beside the one it replaced", () => {
  const before = step("Hash Join", [step("Seq Scan", [], { detail: "on orders" }), step("Hash")]);
  const after = step("Hash Join", [step("Index Scan", [], { detail: "using ix on orders" }), step("Hash")]);
  assert.deepEqual(shape(alignPlans(before, after)), [
    [0, "Hash Join", "Hash Join"],
    [1, "Seq Scan", "Index Scan"],
    [1, "Hash", "Hash"],
  ]);
});

test("steps of one plan only stand alone, with their children", () => {
  const before = step("Sort", [step("Seq Scan")]);
  const after = step("Sort", [step("Gather", [step("Parallel Seq Scan")]), step("Seq Scan")]);
  assert.deepEqual(shape(alignPlans(before, after)), [
    [0, "Sort", "Sort"],
    [1, null, "Gather"],
    [2, null, "Parallel Seq Scan"],
    [1, "Seq Scan", "Seq Scan"],
  ]);
});

test("different tops still pair, their children too", () => {
  assert.deepEqual(shape(alignPlans(step("A", [step("x")]), step("B", [step("x")]))), [
    [0, "A", "B"],
    [1, "x", "x"],
  ]);
});

test("measures, totals and changes", () => {
  const analyzed = { analyzed: true, root: step("A", [], { cost: 10, actual_ms: 2, loops: 3, self_ms: 6, self_cost: 10 }) };
  const estimated = { analyzed: false, root: step("A", [], { cost: 5, self_cost: 5 }) };
  assert.equal(measureOf(analyzed, analyzed), "time");
  assert.equal(measureOf(analyzed, estimated), "cost");
  assert.equal(total(analyzed, "time"), 6);
  assert.equal(total(estimated, "cost"), 5);
  assert.equal(total(estimated, "time"), null);
  assert.equal(own(analyzed.root, "time"), 6);
  assert.equal(own(null, "cost"), null);
  assert.equal(change(10, 5), -0.5);
  assert.equal(change(0, 5), null);
  assert.equal(change(null, 5), null);
});

test("the graph puts leaves in columns and parents over their children", () => {
  const plan = step("Join", [step("Scan a"), step("Hash", [step("Scan b")])]);
  const { boxes, edges, width, height } = layoutPlan(plan);
  const at = Object.fromEntries(boxes.map((b) => [b.path, b]));
  const col = BOX.width + BOX.gapX;
  assert.equal(at["0.0"].x, 0);
  assert.equal(at["0.1.0"].x, col);
  assert.equal(at["0.1"].x, col);
  assert.equal(at["0"].x, col / 2);
  assert.equal(at["0.1.0"].y, 2 * (BOX.height + BOX.gapY));
  assert.deepEqual(edges, [["0", "0.0"], ["0", "0.1"], ["0.1", "0.1.0"]]);
  assert.equal(width, 2 * col - BOX.gapX);
  assert.equal(height, 3 * (BOX.height + BOX.gapY) - BOX.gapY);
});
