// BUG SAFE-05: platypus-mcp passes the output of a failing password command to the agent.
// A connection whose password comes from a command (a vault CLI, a script) that fails after
// printing something on stderr: the tool error returned to the agent quotes that output
// ("password command: exit code: 3: <what it printed>"), and the query history stores it.
// Such output can hold a secret (a token, the password itself, a vault path).
// connect::config_of adds only the command's text to the Redactor, not what it printed.
// Steps: profile with a PostgreSQL connection, password_command = "echo QaLeakedOutput-5521
// 1>&2 && exit 3"; MCP run_query and list_schemas on it.
// Expected: the agent learns that the password command failed (and its exit code), never
// what it printed. Actual: "password command: exit code: 3: QaLeakedOutput-5521".
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { noMcp, startMcp } from "../mcp.mjs";

const PRINTED = "QaLeakedOutput-5521";

export const skip = noMcp;

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000471"
name = "Vaulted PG"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
password_command = "echo ${PRINTED} 1>&2 && exit 3"
`,
  );
}

export default async function (_page, { dir }) {
  const mcp = await startMcp(dir);
  try {
    const query = await mcp.call("run_query", { connection: "Vaulted PG", sql: "SELECT 1" });
    const schemas = await mcp.call("list_schemas", { connection: "Vaulted PG" });
    assert.ok(query.error && schemas.error, "the password command failed: both calls are errors");
    assert.match(query.text, /password command/);
    assert.ok(!query.text.includes(PRINTED), `run_query error quotes the command's output: ${query.text}`);
    assert.ok(!schemas.text.includes(PRINTED), `list_schemas error quotes the command's output: ${schemas.text}`);
  } finally {
    mcp.close();
  }
  const history = new DatabaseSync(join(dir, "history.db"));
  const errors = history.prepare("SELECT error FROM history").all().map((r) => r.error ?? "");
  history.close();
  assert.ok(!errors.some((e) => e.includes(PRINTED)), `the history keeps the output: ${errors}`);
}
