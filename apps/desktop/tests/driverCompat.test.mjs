// Unit tests of the driver compatibility check (src/drivers/compat.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { compatTally, compatTargets, mergeVersions } from "../src/drivers/compat.ts";

const base = { id: "1", name: "c", driver: "snowflake", host: "h", port: null, database: "d", user: "u", save_password: true };

test("only the connections of the driver are tried, and those that ask for a password are left alone", () => {
  const saved = { ...base, id: "a" };
  const asks = { ...base, id: "b", save_password: false };
  const other = { ...base, id: "c", driver: "trino" };
  const { run, skipped } = compatTargets([saved, asks, other], "snowflake");
  assert.deepEqual(run.map((c) => c.id), ["a"]);
  assert.deepEqual(skipped.map((c) => c.id), ["b"]);
  assert.deepEqual(compatTargets([], "snowflake"), { run: [], skipped: [] });
});

test("the tally counts the finished probes only", () => {
  assert.deepEqual(
    compatTally([
      { state: "done", summary: "x", ms: 1 },
      { state: "failed", message: "no" },
      { state: "failed", message: "no" },
      { state: "skipped" },
      { state: "running" },
    ]),
    { done: 1, failed: 2, skipped: 1 },
  );
});

test("versions only the disk has follow the registry's", () => {
  assert.deepEqual(mergeVersions(["v2", "v1"], ["v1", "v0.5"]), ["v2", "v1", "v0.5"]);
  assert.deepEqual(mergeVersions([], ["v1"]), ["v1"]);
});
