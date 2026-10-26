// BUG SAFE-06: run_query with format=json writes 64-bit integers as bare JSON numbers, so an
// agent's JSON parser (JavaScript, and many others that read numbers as doubles) gets a
// different value past 2^53: 9007199254740993 reads as 9007199254740992, an id that points
// at another row. Decimals are already strings ("1.5"); big integers are not.
// Steps: SQLite connection; MCP run_query "SELECT 9007199254740993 AS big" with format json.
// Expected: the value survives JSON.parse (as a string past 2^53, or always for int64).
// Actual: rows [[9007199254740993]] as a number; JSON.parse gives 9007199254740992.
// Suspected: crates/platypus-mcp/src/output.rs, Cell::Number written as a JSON number.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { noMcp, startMcp } from "../mcp.mjs";

export const skip = noMcp;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "ids.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (9007199254740993);");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000472"
name = "Ids db"
driver = "sqlite"
database = '${join(dir, "ids.db")}'
save_password = false
`,
  );
}

export default async function (_page, { dir }) {
  const mcp = await startMcp(dir);
  try {
    const r = await mcp.call("run_query", { connection: "Ids db", sql: "SELECT id FROM t", format: "json" });
    assert.equal(r.error, false, r.text);
    const value = JSON.parse(r.text).rows[0][0];
    assert.equal(String(value), "9007199254740993", `the agent reads ${value}`);
  } finally {
    mcp.close();
  }
}
