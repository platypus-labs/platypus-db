// Unit tests of "Test connections" (src/connectionTest.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { tally, testable } from "../src/connectionTest.ts";

const base = { id: "1", name: "c", driver: "postgres", host: "h", port: null, database: "d", user: "u", save_password: false };

test("a connection that asks for its password each time is not tried", () => {
  assert.equal(testable(base), false);
  assert.equal(testable({ ...base, password_command: "   " }), false);
});

test("files, saved passwords, commands and .pgpass are tried", () => {
  assert.equal(testable({ ...base, driver: "sqlite" }), true);
  assert.equal(testable({ ...base, driver: "duckdb" }), true);
  assert.equal(testable({ ...base, save_password: true }), true);
  assert.equal(testable({ ...base, password_command: "op read op://v/db/password" }), true);
  assert.equal(testable({ ...base, pgpass: true }), true);
});

test("the summary counts the finished tests only", () => {
  const server = { product: "PostgreSQL", version: "17", encrypted: false };
  assert.deepEqual(
    tally([
      { state: "done", server, ms: 3 },
      { state: "done", server, ms: 5 },
      { state: "failed", message: "refused" },
      { state: "skipped" },
      { state: "running" },
      { state: "queued" },
    ]),
    { done: 2, failed: 1, skipped: 1 },
  );
  assert.deepEqual(tally([]), { done: 0, failed: 0, skipped: 0 });
});
