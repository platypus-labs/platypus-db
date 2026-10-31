// Unit tests of the status of a transfer job (src/transfer/jobStatus.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { afterEvent } from "../src/transfer/jobStatus.ts";

const running = { state: "running", id: 3, rows: 0 };

test("progress counts the rows while the job runs", () => {
  assert.deepEqual(afterEvent(running, { type: "progress", rows: 500 }), { ...running, rows: 500 });
  const done = { state: "done", rows: 9, warnings: [] };
  assert.equal(afterEvent(done, { type: "progress", rows: 500 }), done, "a late event changes nothing");
  assert.deepEqual(afterEvent(running, { type: "done", rows: 600, elapsed_ms: 1 }), { state: "done", rows: 600, warnings: [] });
});

test("a failed or stopped job keeps the rows written before", () => {
  const busy = afterEvent(running, { type: "progress", rows: 500 });
  assert.deepEqual(afterEvent(busy, { type: "failed", error: { message: "E11000" } }), { state: "failed", message: "E11000", rows: 500 });
  assert.deepEqual(afterEvent(busy, { type: "cancelled" }), { state: "cancelled", rows: 500 });
  assert.deepEqual(afterEvent(running, { type: "cancelled" }), { state: "cancelled", rows: 0 });
});

test("the count a failure or a stop carries wins over a lagging progress", () => {
  const busy = afterEvent(running, { type: "progress", rows: 1 });
  assert.deepEqual(afterEvent(busy, { type: "failed", error: { message: "no such table" }, rows: 2 }), { state: "failed", message: "no such table", rows: 2 });
  assert.deepEqual(afterEvent(busy, { type: "cancelled", rows: 5 }), { state: "cancelled", rows: 5 });
  assert.deepEqual(afterEvent(running, { type: "failed", error: { message: "x" }, rows: 0 }), { state: "failed", message: "x", rows: 0 });
});

test("a file's progress keeps the bytes read", () => {
  const reading = afterEvent(running, { type: "progress", rows: 2, bytes: [10, 40] });
  assert.deepEqual(reading, { ...running, rows: 2, bytes: [10, 40] });
  assert.deepEqual(afterEvent(reading, { type: "done", rows: 8, elapsed_ms: 1 }), { state: "done", rows: 8, warnings: [] });
});
