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

const cancelled = { kind: "error", error: { kind: "cancelled", message: "Cancelled" } };

test("a run waits for its job until the job is known", () => {
  const g = gate(true);
  assert.equal(g.awaitsJob(), true);
  g.settle(started);
  assert.equal(g.awaitsJob(), true, "events do not give the job");
  g.jobKnown();
  assert.equal(g.awaitsJob(), false);
});

test("a backend that never answers is ended by abandon, once", () => {
  const g = gate(true);
  g.abandon(cancelled);
  assert.deepEqual(g.resolved, [cancelled]);
  assert.equal(g.awaitsJob(), false);
  assert.equal(g.abandoned(), true);
  // A late answer changes nothing.
  g.settle(finished);
  g.jobKnown();
  g.abandon(affected);
  assert.deepEqual(g.resolved, [cancelled]);
});

test("a terminal status waiting for the job is replaced by abandon", () => {
  const g = gate(true);
  g.settle(affected);
  g.abandon(cancelled);
  g.jobKnown();
  assert.deepEqual(g.resolved, [cancelled]);
});

test("abandon after the run is over does not end it twice", () => {
  const g = gate(true);
  g.jobKnown();
  g.settle(finished);
  g.abandon(cancelled);
  assert.deepEqual(g.resolved, [finished]);
  assert.equal(g.abandoned(), false, "the run ended on its own");
});

test("a run whose job is known is ended by abandon without a terminal status", () => {
  const g = gate(true);
  g.jobKnown();
  g.settle(started);
  g.abandon(cancelled);
  assert.deepEqual(g.resolved, [cancelled]);
});

const stopped = { kind: "error", error: { kind: "cancelled", message: "canceling statement due to user request" } };

test("Cancel before the job is known cancels the job once it is; its own status ends the run", () => {
  const g = gate(true);
  g.requestCancel();
  assert.equal(g.awaitsJob(), true, "the statement may still start");
  assert.equal(g.cancelDue(), false, "no job to cancel yet");
  assert.deepEqual(g.resolved, [], "not reported as cancelled before it is");
  g.jobKnown();
  assert.equal(g.cancelDue(), true);
  g.settle(stopped);
  assert.deepEqual(g.resolved, [stopped]);
  assert.equal(g.cancelDue(), false);
  assert.equal(g.abandoned(), false);
});

test("a statement that ended before Cancel reached it keeps its result", () => {
  const g = gate(true);
  g.settle(affected);
  g.requestCancel();
  g.jobKnown();
  assert.equal(g.cancelDue(), false, "nothing left to cancel");
  assert.deepEqual(g.resolved, [affected]);
});

test("a run that fails to start after Cancel reports its own failure", () => {
  const g = gate(true);
  const refused = { kind: "error", error: { kind: "other", message: "connection refused" } };
  g.requestCancel();
  g.jobKnown();
  g.settle(refused);
  assert.deepEqual(g.resolved, [refused]);
  assert.equal(g.cancelDue(), false);
});

test("Cancel after the run is over has nothing to cancel", () => {
  const g = gate(true);
  g.jobKnown();
  g.settle(finished);
  g.requestCancel();
  assert.equal(g.cancelDue(), false);
  assert.deepEqual(g.resolved, [finished]);
});
