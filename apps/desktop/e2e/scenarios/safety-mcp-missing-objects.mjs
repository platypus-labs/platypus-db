// BUG SAFE-07: platypus-mcp answers describe_table on a table that does not exist, and
// list_objects on a schema that does not exist, with an empty success: the agent reads "a
// table without columns" or "an empty schema" (e.g. after a typo, or with the wrong case on
// PostgreSQL) instead of "not found", and may go on to write SQL on that belief. get_ddl, by
// contrast, says "no object `x` in schema `y`".
// Steps: SQLite connection with table t; MCP describe_table {schema main, table nope} and
// list_objects {schema nope}.
// Expected: tool errors saying the table / schema does not exist.
// Actual: {"columns":[],"indexes":[],...} and [] as successes.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { noMcp, startMcp } from "../mcp.mjs";

export const skip = noMcp;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "a.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY)");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000473"
name = "A db"
driver = "sqlite"
database = '${join(dir, "a.db")}'
save_password = false
`,
  );
}

export default async function (_page, { dir }) {
  const mcp = await startMcp(dir);
  try {
    const ok = await mcp.call("describe_table", { connection: "A db", schema: "main", table: "t" });
    assert.equal(JSON.parse(ok.text).columns.length, 1);
    const table = await mcp.call("describe_table", { connection: "A db", schema: "main", table: "nope" });
    assert.equal(table.error, true, `describe_table of a missing table succeeded: ${table.text}`);
    const schema = await mcp.call("list_objects", { connection: "A db", schema: "nope" });
    assert.equal(schema.error, true, `list_objects of a missing schema succeeded: ${schema.text}`);
  } finally {
    mcp.close();
  }
}
