// Unit tests of the note on results in different time zones (src/analysis/zones.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { mixedZones } from "../src/analysis/zones.ts";

test("results in several time zones say which one SQL shows them in", () => {
  assert.equal(mixedZones([]), null);
  assert.equal(mixedZones(["Asia/Tokyo"]), null);
  assert.deepEqual(mixedZones(["Asia/Tokyo", "Europe/Rome", "UTC"]), { zone: "Asia/Tokyo", others: "Europe/Rome, UTC" });
});
