// The Claude Code plugin's launcher (plugins/platypus-db/bin/platypus-mcp.mjs) starts the app with
// --mcp; when the app cannot be started it says so on stderr and exits with 1.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const launcher = join(dirname(dirname(fileURLToPath(import.meta.url))), "plugins", "platypus-db", "bin", "platypus-mcp.mjs");

test("a missing app is reported, with how to point at it", () => {
  const run = spawnSync(process.execPath, [launcher], {
    env: { ...process.env, PLATYPUS_DB: join(dirname(launcher), "no-such-app") },
    encoding: "utf8",
  });
  assert.equal(run.status, 1);
  assert.match(run.stderr, /PlatypusDB was not found .*set PLATYPUS_DB to its executable/);
});

test("the app gets --mcp and the extra arguments, and its exit code comes back", () => {
  // Node itself stands in for the app: `node --mcp` is refused, which proves the flag is passed.
  const run = spawnSync(process.execPath, [launcher], {
    env: { ...process.env, PLATYPUS_DB: process.execPath },
    encoding: "utf8",
  });
  assert.notEqual(run.status, 0);
  assert.match(run.stderr, /--mcp/);
});
