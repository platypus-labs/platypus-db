// Unit tests of the admin tab's waiting chains, filter and durations (src/admin/sessions.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { activity, blockingChains, matches, seconds } from "../src/admin/sessions.ts";

const session = (id, blocked_by = [], extra = {}) => ({
  id,
  user: "app",
  database: "shop",
  client: null,
  application: null,
  state: "active",
  started: null,
  query: `UPDATE t -- ${id}`,
  query_seconds: 1,
  wait: null,
  blocked_by,
  current: false,
  ...extra,
});

const shape = (chains) => chains.map((c) => [c.session.id, shape(c.waiting)]);

test("no waits, no chains", () => {
  assert.deepEqual(blockingChains([session("1"), session("2")]), []);
});

test("a chain starts at the session that waits for none", () => {
  const chains = blockingChains([session("1"), session("2", ["1"]), session("3", ["2"]), session("4", ["1"])]);
  assert.deepEqual(shape(chains), [["1", [["2", [["3", []]]], ["4", []]]]]);
});

test("a session waiting for two shows under both", () => {
  const chains = blockingChains([session("1"), session("2"), session("3", ["1", "2"])]);
  assert.deepEqual(shape(chains), [
    ["1", [["3", []]]],
    ["2", [["3", []]]],
  ]);
});

test("a deadlock cycle shows once, without looping", () => {
  const chains = blockingChains([session("1", ["2"]), session("2", ["1"])]);
  assert.deepEqual(shape(chains), [["1", [["2", []]]]]);
});

test("a blocker the list does not show is no root", () => {
  assert.deepEqual(blockingChains([session("2", ["99"])]), []);
});

test("the filter looks in every field, ignoring case", () => {
  const s = session("42", [], { user: "Alice", client: "10.0.0.5:5432", query: "SELECT * FROM Orders" });
  assert.ok(matches(s, ""));
  assert.ok(matches(s, "alice"));
  assert.ok(matches(s, "orders"));
  assert.ok(matches(s, "10.0.0"));
  assert.ok(matches(s, "42"));
  assert.ok(!matches(s, "bob"));
  assert.ok(matches({ ...s, user: null }, "orders"));
});

test("durations read as seconds, minutes or hours", () => {
  assert.equal(seconds(null), "");
  assert.equal(seconds(0.25), "0.3 s");
  assert.equal(seconds(12.4), "12 s");
  assert.equal(seconds(65), "1 min 05 s");
  assert.equal(seconds(7380), "2 h 03 min");
});

test("durations round before they choose their unit", () => {
  assert.equal(seconds(9.96), "10 s");
  assert.equal(seconds(59.7), "1 min 00 s");
  assert.equal(seconds(3599.6), "1 h 00 min");
});

test("a session's activity: waiting first, then running, open transaction, idle", () => {
  assert.equal(activity(session("1", ["2"])), "waiting");
  assert.equal(activity(session("1")), "running");
  assert.equal(activity(session("1", [], { query_seconds: null, state: "idle in transaction" })), "transaction");
  assert.equal(activity(session("1", [], { query_seconds: null, state: "idle in transaction (aborted)" })), "transaction");
  assert.equal(activity(session("1", [], { query_seconds: null, state: "Sleep" })), "idle");
});
