// QA3 MCP served by the app (`platypus-db --mcp`, what the Agents page now registers),
// driven over stdio like an agent does, on SQLite and PostgreSQL (docker/compose.yml).
//
// Checks: the handshake and the tool list match platypus-mcp; queries on both databases;
// read-only refusals (DML, DDL, batches, the listed PostgreSQL functions plain, quoted and
// schema-qualified); stdout carries JSON-RPC only; no window is opened (no WebView2 folder,
// no main window handle); stdin closed ends the process with 0, also while a statement runs.
//
// BUG QA3-MCP-01 (high) holds here too: `SELECT U&"\0070g_advisory_lock"(…)` and the
// UESCAPE form get past the read-only check (crates/platypus-mcp/src/access.rs). Collected
// as problems at the end, so the passing parts above still guard the rest.
import assert from "node:assert/strict";
import { spawnSync } from "../proc.mjs";
import { existsSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { connectionToml, dbExec, needs } from "../qa-safe-db.mjs";
import { sqliteToml, startRaw } from "../qa3-mcp-client.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const appExe = process.env.PLATYPUS_E2E_EXE ?? resolve(here, "../../../../target/release/platypus-db.exe");
const LOCK = 4245;

export function setup({ dir }) {
  const file = join(dir, "app-mode.db");
  const db = new DatabaseSync(file);
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'c');");
  db.close();
  let toml = `version = 7\n${sqliteToml("00000000-0000-4000-8000-000000000381", "QA3 app lite", file)}`;
  toml += connectionToml("postgres", "00000000-0000-4000-8000-000000000382", "QA3 app pg");
  writeFileSync(join(dir, "connections.toml"), toml);
}

/** The main window handle of `pid` (0 when it has none). */
function mainWindow(pid) {
  const r = spawnSync("powershell", ["-NoProfile", "-Command", `(Get-Process -Id ${pid}).MainWindowHandle`], { encoding: "utf8" });
  return Number(r.stdout.trim() || "0");
}

const REFUSED = [
  "DELETE FROM qa3_app WHERE id = 1",
  "INSERT INTO qa3_app VALUES (9)",
  "DROP TABLE qa3_app",
  "SELECT 1; DELETE FROM qa3_app",
  "SELECT pg_advisory_lock(4246)",
  'SELECT "pg_advisory_lock"(4246)',
  "SELECT pg_catalog.pg_read_file('PG_VERSION')",
  "SELECT nextval('x')",
];

async function postgres(server, problems) {
  const run = (sql) => server.call("run_query", { connection: "QA3 app pg", sql });
  dbExec("postgres", "DROP TABLE IF EXISTS qa3_app; CREATE TABLE qa3_app (id int PRIMARY KEY); INSERT INTO qa3_app VALUES (1), (2);");
  assert.match((await run("SELECT count(*) AS n FROM qa3_app")).text, /\n2\n/);
  for (const sql of REFUSED) {
    const r = await run(sql);
    assert.ok(r.error, `read-only ran on PostgreSQL: ${sql}: ${r.text}`);
  }
  assert.equal(dbExec("postgres", "SELECT count(*) FROM qa3_app"), "2");

  // QA3-MCP-01: Unicode-escaped identifiers.
  const lock = await run(`SELECT U&"\\0070g_advisory_lock"(${LOCK})`);
  const held = dbExec("postgres", `SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = ${LOCK}`);
  if (!lock.error || held !== "0") problems.push(`QA3-MCP-01: U&"\\0070g_advisory_lock" ran through platypus-db --mcp (locks held: ${held})`);
  const escaped = await run(`SELECT U&"!0070g_ls_dir" UESCAPE '!' ('.')`);
  if (!escaped.error) problems.push("QA3-MCP-01: U&\"!0070g_ls_dir\" UESCAPE '!' ran through platypus-db --mcp");
}

export default async function (_page, { dir }) {
  const webview = join(dir, "webview-mcp");
  const server = startRaw(dir, { exe: appExe, args: ["--mcp"], env: { WEBVIEW2_USER_DATA_FOLDER: webview } });
  const problems = [];
  try {
    const init = await server.initialize("qa3-app-mode");
    assert.equal(init.result?.serverInfo?.name, "platypus-mcp", JSON.stringify(init).slice(0, 300));
    const tools = (await server.request("tools/list", {})).result.tools.map((t) => t.name).sort();
    assert.deepEqual(tools, ["compare_schemas", "describe_table", "estimate_change", "explain", "get_ddl", "get_er_diagram", "get_user_context", "list_connections", "list_objects", "list_open_sessions", "list_schemas", "list_scripts", "profile_table", "propose_sql", "read_script", "run_query", "sample_rows", "search_history", "search_objects", "server_activity", "top_queries"]);

    const list = (await server.call("list_connections")).text;
    assert.match(list, /QA3 app lite/);
    assert.match(list, /QA3 app pg/);
    const run = (sql) => server.call("run_query", { connection: "QA3 app lite", sql });
    assert.match((await run("SELECT count(*) AS n FROM t")).text, /\n3\n/);
    for (const sql of ["DELETE FROM t", "UPDATE t SET v = 'x'", "SELECT 1; DROP TABLE t", "ATTACH 'x.db' AS x"]) {
      const r = await run(sql);
      assert.ok(r.error, `read-only ran on SQLite: ${sql}: ${r.text}`);
    }
    assert.match((await run("SELECT count(*) AS n FROM t")).text, /\n3\n/);

    if ((await needs("postgres")()) === null) await postgres(server, problems);

    // No window: WebView2 never started and the process has no main window.
    assert.equal(existsSync(webview), false, "platypus-db --mcp started WebView2");
    assert.equal(mainWindow(server.child.pid), 0, "platypus-db --mcp opened a window");
    assert.deepEqual(server.notJson, [], "stdout carries JSON-RPC only");

    // stdin closed ends the process, also with a statement still running.
    server.post("tools/call", {
      name: "run_query",
      arguments: {
        connection: "QA3 app lite",
        sql: "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT count(*) FROM c",
        timeout_seconds: 60,
      },
    });
    await new Promise((r) => setTimeout(r, 500));
    const exit = await server.closeStdin(15_000);
    assert.deepEqual(exit, { code: 0, signal: null }, `exit on stdin close; stderr ${server.stderr.join("").slice(-500)}`);
  } finally {
    spawnSync("taskkill", ["/pid", String(server.child.pid), "/t", "/f"], { stdio: "ignore" });
  }
  assert.deepEqual(problems, []);
}
