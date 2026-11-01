// Unit tests of sizes and dated file names (src/format.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { datedFileName, formatBytes, formatShare } from "../src/format.ts";

test("a tiny share above zero does not read 0%", () => {
  assert.equal(formatShare(0, "en-US"), "0%");
  assert.equal(formatShare(1 / 5000, "en-US"), "<0.1%");
  assert.equal(formatShare(0.0333, "en-US"), "3.3%");
  assert.equal(formatShare(1, "en-US"), "100%");
});

test("sizes in the largest unit under 1024 of them", () => {
  assert.equal(formatBytes(512), "512 B");
  assert.equal(formatBytes(1536), "1.5 KB");
  assert.equal(formatBytes(23 * 2 ** 20), "23 MB");
});

test("a dated file name without the characters systems refuse", () => {
  const day = new Date("2026-10-29T12:00:00Z");
  assert.equal(datedFileName("shop", "dump", day), "shop-2026-10-29.dump");
  assert.equal(datedFileName("my db: prod/eu", "sql", day), "my_db_prod_eu-2026-10-29.sql");
  assert.equal(datedFileName("", "sql", day), "backup-2026-10-29.sql");
});
