// qa4-safe-1 (high). The run guard (UPDATE/DELETE without WHERE) does not look inside a data-modifying CTE:
// `WITH d AS (DELETE FROM t RETURNING *) SELECT count(*) FROM d` deletes every row of t with no
// confirmation in the editor, and for an agent on a read_write connection (with "Safety: ask" the agent
// cannot answer, so unfiltered changes are refused) it just runs. The statement is a SELECT to the guard,
// while the read-only check already knows it writes (agents-readonly-postgres).
// Steps: PostgreSQL, table t of 3 rows. (1) editor: run the statement above and the UPDATE form;
//   (2) MCP, connection read_write, safety.unfiltered = "ask", confirm_writes off: run_query the same.
// Expected: the editor opens "Change every row?"; the agent is refused ("changes every row of `t`: add a WHERE").
// Actual: both run, t is empty / all rows updated. `UPDATE t SET .. ` and `DELETE FROM t` alone are guarded.
// Code: crates/platypus-sql (analysis of the statement for the unfiltered check: the kind of a `WITH`
//   statement is taken from its final SELECT; DML inside the CTE bodies is not examined).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { connectionToml, dbExec, needs, openEditor, runOrGuard } from "../safety-helpers.mjs";
import { noMcp, startClient } from "../mcp-client.mjs";

export const skip = async () => noMcp() ?? (await needs("postgres")());

const seed = () => dbExec("postgres", "DROP TABLE IF EXISTS qa4_cte; CREATE TABLE qa4_cte (id int PRIMARY KEY, v text); INSERT INTO qa4_cte VALUES (1,'a'),(2,'b'),(3,'c');");
const rows = () => dbExec("postgres", "SELECT coalesce(string_agg(id || v, ',' ORDER BY id), '') FROM qa4_cte");

const STATEMENTS = [
  "WITH d AS (DELETE FROM qa4_cte RETURNING *) SELECT count(*) FROM d",
  "WITH u AS (UPDATE qa4_cte SET v = 'y' RETURNING *) SELECT count(*) FROM u",
];

export function setup({ dir }) {
  seed();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${connectionToml("postgres", "00000000-0000-4000-8000-000000000491", "QA4 pg cte", 'agent_access = "read_write"\n')}`);
  writeFileSync(join(dir, "settings.toml"), '[agents]\nconfirm_writes = false\n[safety]\nunfiltered = "ask"\n');
}

export default async function (page, { dir }) {
  const problems = [];
  try {
    await openEditor(page, "QA4 pg cte");
    {
      const sql = STATEMENTS[0];
      seed();
      const r = await runOrGuard(page, sql);
      if (!r.startsWith("guard:")) problems.push(`editor ran without asking: ${sql} (rows now: ${rows() || "none"})`);
    }
    const mcp = await startClient(dir, "qa4-cte");
    try {
      for (const sql of STATEMENTS) {
        seed();
        const r = await mcp.call("run_query", { connection: "QA4 pg cte", sql });
        if (!r.error) problems.push(`agent ran: ${sql} (rows now: ${rows() || "none"})`);
      }
    } finally {
      mcp.kill();
    }
  } finally {
    dbExec("postgres", "DROP TABLE IF EXISTS qa4_cte");
  }
  assert.deepEqual(problems, []);
}

