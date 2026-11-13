// qa4-mcp-1 (high, secret leak). A statement an agent sends with a password in it is stored as typed in
// the agents' call log (agent-calls.db: shown on Settings > Agents and in its CSV export) and in the
// query history (history.db), and `search_history` hands it to every agent that reads the history.
// The app masks passwords (`platypus_sql::security::mask_secrets`) only in its own query log; the
// agent path never calls it. It leaks whether the statement ran or was refused.
// Steps: PostgreSQL connection with agent access read_write; run_query
//   `CREATE ROLE qa4_leak LOGIN PASSWORD 'Sup3rS3cretQA4'` (refused here: the app is not open; same with the
//   app and a granted write); then search_history, and read agent-calls.db and history.db of the profile.
// Expected: none of them holds `Sup3rS3cretQA4` (PASSWORD '********' as the app's query log shows it).
// Actual: all three show the password in clear.
// Code: crates/platypus-mcp/src/server.rs `Call::new` / `record` (arguments stored as given) and
//   crates/platypus-mcp/src/agent.rs `record` (sql stored as given); neither calls security::mask_secrets.
import assert from "node:assert/strict";
import { readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { connectionToml, needs } from "../safety-helpers.mjs";
import { noMcp, startMcp } from "../mcp.mjs";

export const skip = async () => noMcp() ?? (await needs("postgres")());

const SECRET = "Sup3rS3cretQA4";

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${connectionToml("postgres", "00000000-0000-4000-8000-000000000452", "QA4 pg rw", 'agent_access = "read_write"\n')}`);
}

export default async function (_page, { dir }) {
  const mcp = await startMcp(dir, "qa4-agent");
  const problems = [];
  try {
    await mcp.call("run_query", { connection: "QA4 pg rw", sql: `CREATE ROLE qa4_leak LOGIN PASSWORD '${SECRET}'` });
    const history = await mcp.call("search_history", { connection: "QA4 pg rw" });
    if (history.text.includes(SECRET)) problems.push("search_history returns the password");
  } finally {
    mcp.close();
  }
  await new Promise((r) => setTimeout(r, 300));
  for (const file of readdirSync(dir).filter((f) => f.endsWith(".db"))) {
    const db = new DatabaseSync(join(dir, file));
    try {
      for (const { name } of db.prepare("SELECT name FROM sqlite_master WHERE type = 'table'").all()) {
        if (JSON.stringify(db.prepare(`SELECT * FROM "${name}"`).all()).includes(SECRET)) problems.push(`${file}.${name} holds the password`);
      }
    } finally {
      db.close();
    }
  }
  assert.deepEqual(problems, []);
}
