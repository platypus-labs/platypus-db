// Unit tests of when agent proposals ask before opening (src/agents/flood.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { FEW, RECENT_MS, floods, lately } from "../src/agents/flood.ts";

test("a few proposals open, more ask first", () => {
  const now = 1_000_000;
  assert.equal(floods([], 1, now), false);
  assert.equal(floods([], FEW, now), false);
  assert.equal(floods([], FEW + 1, now), true);
  assert.equal(floods([], 300, now), true);
  // Those opened in the last minute count, older ones do not.
  assert.equal(floods([now - 10, now - 20, now - 30], 1, now), true);
  assert.equal(floods([now - RECENT_MS, now - RECENT_MS - 1, now - 2 * RECENT_MS], 1, now), false);
  assert.deepEqual(lately([now - RECENT_MS, now - 5, now - RECENT_MS + 1], now), [now - 5, now - RECENT_MS + 1]);
});