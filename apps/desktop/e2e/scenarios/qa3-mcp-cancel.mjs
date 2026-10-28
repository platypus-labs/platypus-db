// QA3 MCP cancellation and timeouts, on SQLite files the open app also has.
//
// BUG QA3-MCP-03: `notifications/cancelled` does not stop the running statement. An agent
// (Claude Code on Esc, Codex on interrupt) cancels a run_query; the statement goes on until
// its own timeout_seconds, and every later call on that connection waits behind it: with
// timeout_seconds 60, the next `SELECT 2` answered after about 59 s.
// Steps: run_query with an endless recursive CTE and timeout_seconds 60, cancel it after
// 1 s, run `SELECT 2` on the same connection.
// Expected: the statement is cancelled (as on a timeout) and `SELECT 2` answers within a
// few seconds. Actual: `SELECT 2` answers after ~59 s. The server's run_query takes no
// cancellation token (server.rs run_query; query::run only races the timeout).
//
// Passing parts (regression guards): a slow statement on one connection does not hold the
// others; timeout_seconds cancels and the connection works afterwards; closing stdin
// during a statement ends the server.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { noMcp, sqliteToml, startClient } from "../qa3-mcp-client.mjs";

export const skip = noMcp;

const ENDLESS = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT count(*) FROM c";

export function setup({ dir }) {
  let toml = "version = 7\n";
  for (const [n, name] of [[1, "QA3 slow"], [2, "QA3 other"]]) {
    const file = join(dir, `cancel${n}.db`);
    const db = new DatabaseSync(file);
    db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);");
    db.close();
    toml += sqliteToml(`00000000-0000-4000-8000-00000000031${n}`, name, file);
  }
  writeFileSync(join(dir, "connections.toml"), toml);
}

const seconds = (from) => (Date.now() - from) / 1000;

async function timeouts(mcp) {
  const started = Date.now();
  const slow = await mcp.call("run_query", { connection: "QA3 slow", sql: ENDLESS, timeout_seconds: 1 });
  assert.ok(slow.error, slow.text);
  assert.match(slow.text, /cancelled after 1s/);
  assert.ok(seconds(started) < 10, `the 1 s timeout took ${seconds(started)} s`);
  assert.match((await mcp.call("run_query", { connection: "QA3 slow", sql: "SELECT 'alive' AS s" })).text, /alive/);
}

async function otherConnectionsGoOn(mcp) {
  const slow = mcp.post("tools/call", { name: "run_query", arguments: { connection: "QA3 slow", sql: ENDLESS, timeout_seconds: 8 } });
  await new Promise((r) => setTimeout(r, 500));
  const started = Date.now();
  const other = await mcp.call("run_query", { connection: "QA3 other", sql: "SELECT 'other' AS s" });
  assert.match(other.text, /other/);
  assert.ok(seconds(started) < 5, `another connection waited ${seconds(started)} s behind a slow one`);
  const ping = Date.now();
  await mcp.request("ping");
  assert.ok(seconds(ping) < 2, "ping waits behind a statement");
  const done = await mcp.response(slow, 30_000);
  assert.match(done.result.content[0].text, /cancelled after 8s/);
}

async function cancelNotification(mcp) {
  const id = mcp.post("tools/call", { name: "run_query", arguments: { connection: "QA3 slow", sql: ENDLESS, timeout_seconds: 60 } });
  await new Promise((r) => setTimeout(r, 1000));
  mcp.send({ jsonrpc: "2.0", method: "notifications/cancelled", params: { requestId: id, reason: "user pressed Esc" } });
  const started = Date.now();
  const next = await mcp.call("run_query", { connection: "QA3 slow", sql: "SELECT 2 AS two" }, 90_000);
  assert.match(next.text, /two/);
  assert.ok(seconds(started) < 10, `after a cancelled run_query, the next call on its connection waited ${seconds(started)} s (the cancelled statement kept running)`);
}

export default async function (_page, { dir }) {
  await closeWhileBusy(dir);
  const mcp = await startClient(dir, "qa3-cancel");
  try {
    await timeouts(mcp);
    await otherConnectionsGoOn(mcp);
    await cancelNotification(mcp);
  } finally {
    mcp.kill();
  }
}

/** Closing stdin while a statement runs ends the server soon, with success. */
async function closeWhileBusy(dir) {
  const busy = await startClient(dir, "qa3-cancel-close");
  busy.post("tools/call", { name: "run_query", arguments: { connection: "QA3 slow", sql: ENDLESS, timeout_seconds: 120 } });
  await new Promise((r) => setTimeout(r, 800));
  const started = Date.now();
  const exit = await busy.closeStdin(30_000);
  busy.kill();
  assert.ok(exit !== null, "the server kept running after stdin closed");
  assert.equal(exit.code, 0);
  assert.ok(seconds(started) < 15, `closing stdin took ${seconds(started)} s to end the server`);
}
