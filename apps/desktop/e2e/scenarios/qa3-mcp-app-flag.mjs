// QA3 MCP served by the app itself: `platypus-db --mcp` (roadmap group P, "Un solo
// eseguibile anche per gli agenti") answers MCP on stdio without opening a window, on the
// profile of PLATYPUS_DATA_DIR, with the same tools as platypus-mcp, keeps stdout to
// JSON-RPC and exits 0 when stdin closes; the app's normal start (this run's window) is
// unaffected.
//
// QA3-MCP-00 (resolved): the app ignored `--mcp` until group P was merged into this round.
// qa3-mcp-app-mode.mjs drives the same mode further (PostgreSQL, read-only, no window).
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { sqliteToml, startRaw } from "../qa3-mcp-client.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const appExe = process.env.PLATYPUS_E2E_EXE ?? resolve(here, "../../../../target/release/platypus-db.exe");

export function setup({ dir }) {
  const file = join(dir, "flag.db");
  const db = new DatabaseSync(file);
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1), (2);");
  db.close();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${sqliteToml("00000000-0000-4000-8000-000000000371", "QA3 flag", file)}`);
}

export default async function (_page, { dir }) {
  const server = startRaw(dir, { exe: appExe, args: ["--mcp"], env: { WEBVIEW2_USER_DATA_FOLDER: join(dir, "webview-mcp") } });
  try {
    const init = await server.initialize("qa3-app-flag").catch((e) => ({ error: e.message }));
    assert.ok(init.result, `platypus-db --mcp does not answer initialize: ${JSON.stringify(init).slice(0, 300)}`);
    const tools = (await server.request("tools/list", {})).result.tools.map((t) => t.name).sort();
    assert.ok(tools.includes("run_query") && tools.includes("list_connections"), tools.join(","));
    assert.match((await server.call("run_query", { connection: "QA3 flag", sql: "SELECT count(*) AS n FROM t" })).text, /\n2\n/);
    assert.match((await server.call("run_query", { connection: "QA3 flag", sql: "DELETE FROM t WHERE id = 1" })).text, /read-only/);
    assert.deepEqual(server.notJson, [], "stdout carries JSON-RPC only");
    const exit = await server.closeStdin(15_000);
    assert.deepEqual(exit, { code: 0, signal: null });
  } finally {
    spawnSync("taskkill", ["/pid", String(server.child.pid), "/t", "/f"], { stdio: "ignore" });
  }
}
