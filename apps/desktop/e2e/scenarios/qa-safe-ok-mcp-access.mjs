// Regression guard (passes): platypus-mcp on a profile of SQLite connections. The tool list;
// list_connections hides "off" connections and caps production and read-only ones at read
// only; writes are refused there (also by id and by name in another case), and on a
// read_write connection UPDATE/DELETE without WHERE, BEGIN and several statements per call
// are refused while a filtered UPDATE commits; a change of access in connections.toml applies
// at the next call; every call lands in the history with the client's name, cleaned.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { noMcp, startMcp } from "../mcp.mjs";

export const skip = noMcp;

const connections = (dir, extra = {}) => {
  const c = (n, name, more) => `
[[connections]]
id = "00000000-0000-4000-8000-00000000050${n}"
name = "${name}"
driver = "sqlite"
database = '${join(dir, "a.db")}'
save_password = false
${more}
`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n${c(1, "RW", extra.rw ?? 'agent_access = "read_write"')}${c(2, "Prod", 'connection_type = "prod"\nagent_access = "read_write"')}${c(3, "Locked", 'read_only = true\nagent_access = "read_write"')}${c(4, "Hidden", 'agent_access = "off"')}${c(5, "Plain", "")}`,
  );
};

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "a.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a'), (2, 'b');");
  db.close();
  connections(dir);
}

export default async function (_page, { dir }) {
  const mcp = await startMcp(dir, "  qa\u0007agent ");
  const rows = () => {
    const db = new DatabaseSync(join(dir, "a.db"));
    try {
      return db.prepare("SELECT id, v FROM t ORDER BY id").all().map((r) => `${r.id}${r.v}`).join(",");
    } finally {
      db.close();
    }
  };
  try {
    const tools = (await mcp.request("tools/list", {})).result.tools.map((t) => t.name).sort();
    assert.deepEqual(tools, ["compare_schemas", "describe_table", "estimate_change", "explain", "get_ddl", "get_er_diagram", "get_user_context", "list_connections", "list_objects", "list_open_sessions", "list_schemas", "list_scripts", "profile_table", "propose_sql", "read_script", "run_query", "sample_rows", "search_history", "search_objects", "server_activity", "top_queries"]);

    const list = JSON.parse((await mcp.call("list_connections")).text);
    assert.deepEqual(
      list.map((c) => [c.name, c.agent_access]),
      [["RW", "read_write"], ["Prod", "read_only"], ["Locked", "read_only"], ["Plain", "read_only"]],
    );
    for (const c of list) assert.deepEqual(Object.keys(c).sort(), ["agent_access", "database", "driver", "id", "name", "type"]);

    const run = (connection, sql) => mcp.call("run_query", { connection, sql });
    for (const [connection, sql] of [
      ["Prod", "UPDATE t SET v = 'x' WHERE id = 1"],
      ["Locked", "DELETE FROM t WHERE id = 1"],
      ["Plain", "INSERT INTO t VALUES (3, 'c')"],
      ["plain", "CREATE TABLE qa_safe_x (a int)"],
      ["00000000-0000-4000-8000-000000000502", "DROP TABLE t"],
    ]) {
      const r = await run(connection, sql);
      assert.ok(r.error && /read-only for agents/.test(r.text), `${connection}: ${sql}: ${r.text}`);
    }
    for (const name of ["Hidden", "hidden", "00000000-0000-4000-8000-000000000504"]) {
      const r = await run(name, "SELECT 1");
      assert.ok(r.error && /no connection named/.test(r.text), r.text);
    }
    assert.match((await run("RW", "DELETE FROM t")).text, /every row of `t`/);
    assert.match((await run("RW", "UPDATE t SET v = 'q'")).text, /every row of `t`/);
    assert.match((await run("RW", "BEGIN")).text, /commits on its own/);
    assert.match((await run("RW", "SELECT 1; DELETE FROM t WHERE id = 1")).text, /one statement at a time/);
    assert.equal(rows(), "1a,2b");
    const ok = await run("RW", "UPDATE t SET v = 'z' WHERE id = 2");
    assert.equal(ok.error, false, ok.text);
    assert.equal(rows(), "1a,2z");

    // Access changed in the app: applies at the next call.
    connections(dir, { rw: 'agent_access = "off"' });
    assert.match((await run("RW", "SELECT 1")).text, /no connection named/);
    connections(dir, { rw: 'agent_access = "read_write"\nconnection_type = "prod"' });
    assert.match((await run("RW", "UPDATE t SET v = 'y' WHERE id = 2")).text, /read-only for agents/);
    assert.equal(rows(), "1a,2z");
  } finally {
    mcp.close();
  }
  const history = new DatabaseSync(join(dir, "history.db"));
  const origins = new Set(history.prepare("SELECT origin FROM history").all().map((r) => r.origin));
  history.close();
  assert.deepEqual([...origins], ["qaagent"]);
}
