// Unit tests of when a statement's run is over (src/components/runGate.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { runGate } from "../src/components/runGate.ts";

const affected = { kind: "affected", rows: 1, elapsedMs: 1 };
const started = { kind: "rows", finished: false, complete: true, elapsedMs: 1 };
const finished = { kind: "rows", finished: true, complete: true, elapsedMs: 2 };

function gate(limited) {
  const resolved = [];
  return { resolved, ...runGate(limited, (s) => resolved.push(s)) };
}

test("a statement that ends before its job is known resolves once the job is", () => {
  const g = gate(true);
  g.settle(affected);
  assert.deepEqual(g.resolved, []);
  g.jobKnown();
  assert.deepEqual(g.resolved, [affected]);
});

test("a known job resolves at the first terminal status, once", () => {
  const g = gate(true);
  g.jobKnown();
  g.settle(started);
  assert.deepEqual(g.resolved, [], "rows still coming with a limit");
  g.settle(finished);
  g.settle(affected);
  g.jobKnown();
  assert.deepEqual(g.resolved, [finished]);
});

test("without a limit the first rows end the run", () => {
  const g = gate(false);
  g.settle(started);
  g.jobKnown();
  assert.deepEqual(g.resolved, [started]);
});

test("a run that fails to start resolves with its error", () => {
  const g = gate(true);
  const error = { kind: "error", error: { kind: "other", message: "no" } };
  g.jobKnown();
  g.settle(error);
  assert.deepEqual(g.resolved, [error]);
});

test("a skipped statement is not the end", () => {
  const g = gate(true);
  g.jobKnown();
  g.settle({ kind: "skipped" });
  assert.deepEqual(g.resolved, []);
});
