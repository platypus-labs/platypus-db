// qa4-mcp-3 (high, secret leak). A read-only agent on Redis reads the server's own password.
// Steps: Redis connection (password from a password command), agent access read_only; run_query
//   `CONFIG GET requirepass`, `CONFIG GET *`, `ACL LIST` / `ACL GETUSER default`.
// Expected: the connection's password is masked (`***`) as it is in errors, or the commands that
//   reveal credentials (CONFIG GET of requirepass / masterauth / masteruser, ACL GETUSER / LIST, which
//   show password hashes) are not "commands that change nothing" an agent may run.
// Actual: `CONFIG GET requirepass` returns the password in clear (the one the app connected with, and
//   the one every other client of that server uses), `ACL GETUSER` returns the SHA-256 of each password.
// Code: crates/platypus-driver-redis/src/command.rs READ_SUBCOMMANDS lists CONFIG GET, ACL GETUSER/LIST
//   as reads; crates/platypus-mcp/src/agent.rs applies the Redactor (redact.rs) to errors only, never
//   to the rows of a result.
import assert from "node:assert/strict";
import { noMcp, startClient } from "../mcp-client.mjs";
import { connection, saveConnections, skip as redisSkip } from "../redis-helpers.mjs";

export const skip = async () => noMcp() ?? (await redisSkip());
export const setup = ({ dir }) => saveConnections(dir, connection(1, "QA4 redis", 0));

export default async function (_page, { dir }) {
  const mcp = await startClient(dir, "qa4-redis");
  const problems = [];
  try {
    for (const sql of ["CONFIG GET requirepass", "CONFIG GET *", "CONFIG GET pass*"]) {
      const r = await mcp.call("run_query", { connection: "QA4 redis", sql });
      if (!r.error && /\bplatypus\b/.test(r.text.replace(/^.*(save|dir|logfile).*$/gm, ""))) problems.push(`${sql} returned the server password`);
    }
    for (const sql of ["ACL GETUSER default", "ACL LIST"]) {
      const r = await mcp.call("run_query", { connection: "QA4 redis", sql });
      if (!r.error && /[0-9a-f]{64}/.test(r.text)) problems.push(`${sql} returned password hashes`);
    }
  } finally {
    mcp.kill();
  }
  assert.deepEqual(problems, []);
}
