// Unit tests of the per-key queue behind the catalog's writes (src/catalog/queue.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { keyedQueue } from "../src/catalog/queue.ts";

const later = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

test("tasks of one key run one after the other, in the order asked", async () => {
  const queue = keyedQueue();
  const log = [];
  const task = (name, ms) => async () => {
    log.push(`${name} start`);
    await later(ms);
    log.push(`${name} end`);
    return name;
  };
  const results = await Promise.all([queue.run("a", task("slow", 20)), queue.run("a", task("fast", 1))]);
  assert.deepEqual(results, ["slow", "fast"]);
  assert.deepEqual(log, ["slow start", "slow end", "fast start", "fast end"]);
});

test("a failed task reaches its caller and does not block the next one", async () => {
  const queue = keyedQueue();
  const failing = queue.run("a", async () => {
    throw new Error("boom");
  });
  const next = queue.run("a", async () => "ok");
  await assert.rejects(failing, /boom/);
  assert.equal(await next, "ok");
});

test("different keys do not wait for each other", async () => {
  const queue = keyedQueue();
  const log = [];
  const slow = queue.run("a", async () => {
    await later(20);
    log.push("a");
  });
  await queue.run("b", async () => void log.push("b"));
  await slow;
  assert.deepEqual(log, ["b", "a"]);
});

test("a forgotten key starts afresh", async () => {
  const queue = keyedQueue();
  queue.forget("never used");
  assert.equal(await queue.run("a", async () => 1), 1);
  queue.forget("a");
  assert.equal(await queue.run("a", async () => 2), 2);
});
