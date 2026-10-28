// QA3 MCP protocol: `platypus-db --mcp` (what the Agents page registers) driven over stdio as an
// MCP client would, while the app is open on the same profile and SQLite file. Checks the
// handshake, tools/list and its input schemas, every tool with valid input, invalid and
// missing arguments, an unknown tool and method, concurrent requests, stdout carrying only
// JSON-RPC, the server surviving garbage, and a clean exit when stdin closes.
//
// BUG QA3-MCP-04: a line that is not JSON gets no answer. JSON-RPC 2.0 wants a Parse error
// (-32700, id null); the client waits for nothing and cannot tell its request was lost.
// BUG QA3-MCP-05: an unknown command-line argument (`--mcp --bogus`) is taken as
// "serve": the server starts and waits on stdin instead of printing the usage and exiting
// with an error, so a mistyped agent config looks like a hung server.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { mcpArgs, mcpExe, noMcp, sqliteToml, startClient, startRaw } from "../qa3-mcp-client.mjs";

export const skip = noMcp;

const RO = "00000000-0000-4000-8000-000000000301";
const TOOLS = ["describe_table", "get_ddl", "list_connections", "list_objects", "list_schemas", "run_query", "search_objects"];

export function setup({ dir }) {
  const file = join(dir, "qa3.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE customer (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE);
    CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customer(id), total REAL);
    CREATE INDEX orders_customer ON orders(customer_id);
    CREATE VIEW big_orders AS SELECT * FROM orders WHERE total > 100;
    INSERT INTO customer VALUES (1, 'Ada', 'ada@example.com'), (2, 'Bob', NULL);
    INSERT INTO orders VALUES (1, 1, 50), (2, 1, 150), (3, 2, 300);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n${sqliteToml(RO, "QA3 shop", file)}${sqliteToml(
      "00000000-0000-4000-8000-000000000302",
      "QA3 hidden",
      file,
      'agent_access = "off"',
    )}`,
  );
}

async function handshakeAndTools(mcp) {
  const init = mcp.init.result;
  assert.equal(init.serverInfo.name, "platypus-mcp");
  assert.ok(init.capabilities.tools, "the server announces tools");
  assert.match(init.instructions, /list_connections/);
  const listed = (await mcp.request("tools/list", {})).result.tools;
  assert.deepEqual(listed.map((t) => t.name).sort(), TOOLS);
  for (const tool of listed) {
    assert.ok(tool.description?.length > 20, `${tool.name} has a description`);
    assert.equal(tool.inputSchema?.type, "object", `${tool.name} has an object input schema`);
  }
  const runQuery = listed.find((t) => t.name === "run_query").inputSchema;
  assert.deepEqual([...(runQuery.required ?? [])].sort(), ["connection", "sql"]);
  assert.equal((await mcp.request("ping")).result !== undefined, true, "ping answers");
}

async function everyTool(mcp) {
  const conn = { connection: "QA3 shop" };
  const list = JSON.parse((await mcp.call("list_connections")).text);
  assert.deepEqual(list.map((c) => c.name), ["QA3 shop"], "the hidden connection stays hidden");
  assert.equal(list[0].agent_access, "read_only");

  const schemas = JSON.parse((await mcp.call("list_schemas", conn)).text);
  assert.ok(schemas.some((s) => s.name === "main" && s.current));

  const objects = JSON.parse((await mcp.call("list_objects", { ...conn, schema: "main" })).text);
  for (const name of ["customer", "orders", "big_orders"]) assert.ok(objects.some((o) => o.name === name), name);

  const described = JSON.parse((await mcp.call("describe_table", { ...conn, schema: "main", table: "orders" })).text);
  assert.ok(described.columns.find((c) => c.name === "id").primary_key);
  assert.equal(described.foreign_keys.length, 1);
  assert.ok(described.indexes.some((i) => i.name === "orders_customer"));
  const customer = JSON.parse((await mcp.call("describe_table", { ...conn, schema: "main", table: "customer" })).text);
  assert.equal(customer.referenced_by.length, 1, "customer knows orders points at it");

  assert.match((await mcp.call("get_ddl", { ...conn, schema: "main", name: "big_orders" })).text, /CREATE VIEW big_orders/);
  assert.match((await mcp.call("get_ddl", { ...conn, schema: "main", name: "orders", kind: "table" })).text, /CREATE TABLE orders/);

  const found = JSON.parse((await mcp.call("search_objects", { ...conn, text: "ORDER" })).text);
  assert.ok(JSON.stringify(found).includes("big_orders"));

  const rows = await mcp.call("run_query", { ...conn, sql: "SELECT name FROM customer ORDER BY id" });
  assert.equal(rows.error, false, rows.text);
  assert.match(rows.text, /Ada[\s\S]*Bob[\s\S]*2 rows/);
  const json = JSON.parse((await mcp.call("run_query", { ...conn, sql: "SELECT id, total FROM orders ORDER BY id", format: "json" })).text);
  assert.deepEqual(json.rows, [[1, 50], [2, 150], [3, 300]]);
  // By id too, as list_connections shows it.
  assert.equal((await mcp.call("run_query", { connection: RO, sql: "SELECT 1" })).error, false);
}

async function badInput(mcp) {
  const conn = { connection: "QA3 shop" };
  const expectToolError = async (name, args, pattern) => {
    const r = await mcp.call(name, args);
    assert.ok(r.error, `${name} ${JSON.stringify(args)} should fail: ${r.text}`);
    assert.match(r.text, pattern, `${name} ${JSON.stringify(args)}`);
  };
  await expectToolError("run_query", {}, /connection/);
  await expectToolError("run_query", conn, /sql/);
  await expectToolError("run_query", { ...conn, sql: 42 }, /string/);
  await expectToolError("run_query", { ...conn, sql: "SELECT 1", max_rows: -5 }, /max_rows|u64/);
  await expectToolError("run_query", { ...conn, sql: "SELECT 1", max_rows: 0 }, /max_rows must be greater than 0/);
  await expectToolError("run_query", { ...conn, sql: "SELECT 1", timeout_seconds: 0 }, /timeout_seconds/);
  await expectToolError("run_query", { ...conn, sql: "SELECT 1", format: "xml" }, /text|json/);
  await expectToolError("run_query", { ...conn, sql: "  -- only a comment\n" }, /no SQL/);
  await expectToolError("run_query", { connection: "QA3 hidden", sql: "SELECT 1" }, /no connection named|not available/);
  await expectToolError("run_query", { connection: "00000000-0000-4000-8000-000000000302", sql: "SELECT 1" }, /no connection named|not available/);
  await expectToolError("list_objects", conn, /schema/);
  await expectToolError("describe_table", { ...conn, schema: "main", table: "nope" }, /no table or view/);
  await expectToolError("get_ddl", { ...conn, schema: "main", name: "orders", kind: "bogus" }, /unknown object kind/);

  const unknownTool = await mcp.call("drop_everything", {});
  assert.ok(unknownTool.error, "an unknown tool is an error");
  const unknownMethod = await mcp.request("platypus/secrets", {});
  assert.equal(unknownMethod.error?.code, -32601, JSON.stringify(unknownMethod));
}

async function concurrent(mcp) {
  const ids = [];
  for (let i = 0; i < 12; i++) {
    const name = ["run_query", "list_objects", "describe_table"][i % 3];
    const args = { connection: "QA3 shop", sql: `SELECT ${i} AS n`, schema: "main", table: "orders" };
    ids.push([i, name, mcp.post("tools/call", { name, arguments: args })]);
  }
  for (const [i, name, id] of ids) {
    const r = await mcp.response(id, 30_000);
    assert.equal(r.result?.isError, false, `${name} #${i}: ${JSON.stringify(r)}`);
    if (name === "run_query") assert.match(r.result.content[0].text, new RegExp(`\\n${i}\\n`));
  }
}

async function garbage(mcp) {
  mcp.writeLine("{this is not json");
  // The server stays up and answers the next request.
  assert.ok((await mcp.request("ping", undefined, 10_000)).result !== undefined, "alive after garbage");
  const parseError = mcp.lines.find((m) => m.id === null && m.error?.code === -32700);
  assert.ok(parseError, `a line that is not JSON gets a Parse error (-32700, id null); got ${JSON.stringify(mcp.lines.filter((m) => m.id == null))}`);
}

export default async function (_page, { dir }) {
  const mcp = await startClient(dir, "qa3-protocol");
  let failure;
  try {
    await handshakeAndTools(mcp);
    await everyTool(mcp);
    await badInput(mcp);
    await concurrent(mcp);
    assert.deepEqual(mcp.notJson, [], "stdout carries JSON-RPC only");
    assert.deepEqual(
      mcp.lines.filter((m) => m.jsonrpc !== "2.0"),
      [],
      "every message is JSON-RPC 2.0",
    );
    // Closing stdin ends the server, with success.
    const exit = await mcp.closeStdin(15_000);
    assert.deepEqual(exit, { code: 0, signal: null }, `exit after stdin closed: ${JSON.stringify(exit)}`);
  } catch (e) {
    failure = e;
  } finally {
    mcp.kill();
  }
  if (failure) throw failure;

  // The two bugs are checked apart, so that one does not hide the other.
  const problems = [];
  // Garbage on stdin, in a server of its own.
  const raw = await startClient(dir, "qa3-garbage");
  try {
    await garbage(raw);
  } catch (e) {
    problems.push(`QA3-MCP-04: ${e.message}`);
  } finally {
    raw.kill();
  }

  // Asking the version prints it and exits; a mistyped option is an error.
  const version = spawnSync(mcpExe, [...mcpArgs, "--version"], { encoding: "utf8", input: "", timeout: 15_000 });
  assert.equal(version.status, 0);
  assert.match(version.stdout, /^platypus-db --mcp \d+\.\d+\.\d+/);
  const bogus = startRaw(dir, { args: ["--bogus"] });
  const exited = await Promise.race([bogus.exited, new Promise((r) => setTimeout(() => r(null), 5_000))]);
  bogus.kill();
  if (exited === null || exited.code === 0 || !/bogus|usage|unknown/i.test(bogus.stderr.join(""))) {
    problems.push(
      `QA3-MCP-05: \`platypus-db --mcp --bogus\` should exit with an error naming the option; exit ${JSON.stringify(exited)}, stderr ${JSON.stringify(bogus.stderr.join(""))}`,
    );
  }
  assert.deepEqual(problems, []);
}
