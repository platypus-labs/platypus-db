// Unit tests of the saved workspace's cleaning (src/workspace.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { cleanWorkspace } from "../src/workspace.ts";

test("entries that are not tabs are dropped and the active indices follow", () => {
  const a = { kind: "query", connectionId: "c1" };
  const b = { kind: "query", connectionId: "c2" };
  const clean = cleanWorkspace({ tabs: [null, a, 7, { kind: "query" }, b], active: { all: 4, p1: 0, p2: 1 } });
  assert.deepEqual(clean, { tabs: [a, b], active: { all: 1, p2: 0 } });
});

test("a workspace without a list of tabs is no workspace", () => {
  assert.equal(cleanWorkspace(null), null);
  assert.equal(cleanWorkspace({ tabs: "x", active: {} }), null);
  assert.deepEqual(cleanWorkspace({ tabs: [], active: null }), { tabs: [], active: {} });
});