// Unit tests of the queue of agents' changes waiting for the user (src/agents/writes.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { ANSWER_MS, denial, enqueue, estimatedRows, settle, statementParts, timeLeft } from "../src/agents/writes.ts";

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

test("padding cannot push the end of a statement out of sight", () => {
  const tail = statementParts(`UPDATE orders SET status = 'x' WHERE id = 1${"\n".repeat(60)}OR 1 = 1`);
  assert.deepEqual(tail, [{ text: "UPDATE orders SET status = 'x' WHERE id = 1\n" }, { gap: "lines", count: 59 }, { text: "\nOR 1 = 1" }]);
  // Spaces, tabs and invisible ones in a line, other line breaks, blank lines at the end.
  const wide = statementParts(`DELETE FROM t WHERE a = 1${" \t\u00a0\u200b".repeat(10)}OR 1 = 1\r\n\u2028\r\n`);
  assert.deepEqual(wide, [{ text: "DELETE FROM t WHERE a = 1" }, { gap: "spaces", count: 40 }, { text: "OR 1 = 1\n" }, { gap: "lines", count: 3 }]);
  // What a person writes is kept as it is: indentation, one blank line between parts.
  const sql = "UPDATE t\n   SET a = 1\n\n WHERE id = 2";
  assert.deepEqual(statementParts(sql), [{ text: sql }]);
  assert.deepEqual(statementParts(""), []);
});

test("format characters cannot reorder or hide what the dialog shows", () => {
  // A right-to-left override shows `WHERE id = 1` while it runs `1 = di EREHW`.
  const bidi = statementParts("DELETE FROM t ‮WHERE id = 1‬");
  assert.deepEqual(bidi, [{ text: "DELETE FROM t " }, { invisible: "U+202E" }, { text: "WHERE id = 1" }, { invisible: "U+202C" }]);
  // Isolates, marks, zero-width ones and a code point beyond the BMP (a tag character).
  const hidden = statementParts("a⁦b⁩‎‏c​d\u{e0041}");
  assert.deepEqual(hidden, [
    { text: "a" },
    { invisible: "U+2066" },
    { text: "b" },
    { invisible: "U+2069" },
    { invisible: "U+200E" },
    { invisible: "U+200F" },
    { text: "c" },
    { invisible: "U+200B" },
    { text: "d" },
    { invisible: "U+E0041" },
  ]);
  // Inside a long run of padding they are counted with it.
  assert.deepEqual(statementParts(`a${"​".repeat(20)}b`), [{ text: "a" }, { gap: "spaces", count: 20 }, { text: "b" }]);
});