// Unit tests of the queue of agents' changes waiting for the user (src/agents/writes.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { ANSWER_MS, denial, enqueue, estimatedRows, settle, timeLeft } from "../src/agents/writes.ts";

const request = (id) => ({
  id,
  agent: "claude-code",
  session: "s1",
  connection: "Shop",
  statement: "DELETE FROM t WHERE id = 1",
  estimated_rows: 1,
  plan: null,
});

test("requests wait in the order they came, once each", () => {
  let queue = enqueue([], request("a"), 10);
  queue = enqueue(queue, request("b"), 20);
  queue = enqueue(queue, request("a"), 30);
  assert.deepEqual(
    queue.map((w) => [w.request.id, w.since]),
    [
      ["a", 10],
      ["b", 20],
    ],
  );
  assert.deepEqual(
    settle(queue, "a").map((w) => w.request.id),
    ["b"],
  );
  assert.deepEqual(settle(queue, "nope").length, 2);
});

test("the time left to answer counts down to zero", () => {
  assert.equal(timeLeft(0, 0), "5:00");
  assert.equal(timeLeft(0, 1), "5:00");
  assert.equal(timeLeft(0, 61_000), "3:59");
  assert.equal(timeLeft(0, ANSWER_MS - 9_000), "0:09");
  assert.equal(timeLeft(0, ANSWER_MS + 5_000), "0:00");
});

test("estimates read as whole rows", () => {
  assert.equal(estimatedRows(null), null);
  assert.equal(estimatedRows(Number.NaN), null);
  assert.equal(estimatedRows(-1), null);
  assert.equal(estimatedRows(0), 0);
  assert.equal(estimatedRows(0.2), 1);
  assert.equal(estimatedRows(1234.6), 1235);
});

test("a refusal keeps the reason typed, trimmed", () => {
  assert.deepEqual(denial("  wrong table "), { answer: "deny", reason: "wrong table" });
  assert.deepEqual(denial("   "), { answer: "deny", reason: null });
});
