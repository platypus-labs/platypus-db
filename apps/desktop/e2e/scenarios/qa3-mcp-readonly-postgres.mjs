// QA3 MCP read-only enforcement on PostgreSQL (docker/compose.yml), the app open beside.
//
// BUG QA3-MCP-01 (high): a Unicode-escaped quoted identifier gets past the read-only
// check. `SELECT U&"\0070g_advisory_lock"(4243)` is `pg_advisory_lock(4243)` to the server,
// but the check compares the raw token text (`\0070g_advisory_lock`) with its list, so a
// read-only agent takes a session lock that outlives the rolled-back transaction (still
// held by the server's pooled connection afterwards), and reads server files with
// `U&"\0070g_read_file"('PG_VERSION')`. The same opens pg_terminate_backend, lo_export,
// dblink and every other function of the list.
// Expected: refused as `pg_advisory_lock(…)` and `pg_read_file(…)` are. Actual: they run.
// Suspected: crates/platypus-mcp/src/access.rs:299 (outside_effect trims quotes off a
// QuotedIdent but never decodes U&"…" escapes, nor the UESCAPE form).
//
// BUG QA3-MCP-02 (medium): `SELECT pg_stat_reset()` and `pg_stat_reset_shared('bgwriter')`
// run on a read-only connection: they reset the server's statistics (what monitoring and
// the DBA rely on), outside any transaction. Missing from POSTGRES_OUTSIDE
// (access.rs:51), with pg_stat_reset_single_table_counters, pg_stat_statements_reset…
//
// Passing parts (regression guards): DML, DDL, CTE with DML, SELECT FOR UPDATE, COPY,
// LISTEN, nextval, EXPLAIN ANALYZE of a DELETE, the listed functions quoted or
// schema-qualified, multi-statement batches; reads still run.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { connectionToml, dbExec, needs } from "../qa-safe-db.mjs";
import { noMcp, startClient } from "../qa3-mcp-client.mjs";

export const skip = async () => noMcp() ?? (await needs("postgres")());

const LOCK = 4243;

export function setup({ dir }) {
  dbExec(
    "postgres",
    "DROP TABLE IF EXISTS qa3_ro; CREATE TABLE qa3_ro (id int PRIMARY KEY, v text); INSERT INTO qa3_ro VALUES (1, 'a'), (2, 'b'); DROP SEQUENCE IF EXISTS qa3_ro_seq; CREATE SEQUENCE qa3_ro_seq;",
  );
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${connectionToml("postgres", "00000000-0000-4000-8000-000000000321", "QA3 PG ro")}`);
}

const REFUSED = [
  "DELETE FROM qa3_ro WHERE id = 1",
  "UPDATE qa3_ro SET v = 'x' WHERE id = 1",
  "INSERT INTO qa3_ro VALUES (3, 'c')",
  "WITH gone AS (DELETE FROM qa3_ro RETURNING *) SELECT * FROM gone",
  "/* harmless */ DROP TABLE qa3_ro",
  "TRUNCATE qa3_ro",
  "SELECT * FROM qa3_ro FOR UPDATE",
  "COPY qa3_ro TO STDOUT",
  "LISTEN qa3",
  "SELECT nextval('qa3_ro_seq')",
  "EXPLAIN ANALYZE DELETE FROM qa3_ro",
  "SELECT 1; DELETE FROM qa3_ro WHERE id = 2",
  'SELECT "pg_advisory_lock"(4244)',
  "SELECT pg_catalog.pg_terminate_backend(pg_backend_pid())",
  "SELECT PG_READ_FILE('PG_VERSION')",
  "SELECT pg_ls_dir('.')",
  "SET search_path = evil",
  "DO $$ BEGIN DELETE FROM qa3_ro; END $$",
  "CALL some_procedure()",
];

export default async function (_page, { dir }) {
  const mcp = await startClient(dir, "qa3-readonly-pg");
  const run = (sql) => mcp.call("run_query", { connection: "QA3 PG ro", sql });
  const problems = [];
  try {
    for (const sql of REFUSED) {
      const r = await run(sql);
      assert.ok(r.error, `read-only ran: ${sql}: ${r.text}`);
      assert.match(r.text, /read-only|one statement at a time/, sql);
    }
    assert.match((await run("SELECT count(*) AS n FROM qa3_ro")).text, /\n2\n/);
    assert.match((await run("WITH x AS (SELECT 1 AS a) SELECT a FROM x")).text, /\n1\n/);
    assert.equal(dbExec("postgres", "SELECT count(*) FROM qa3_ro"), "2");
    assert.equal(dbExec("postgres", "SELECT last_value FROM qa3_ro_seq"), "1");

    // QA3-MCP-01: the same functions behind Unicode escapes.
    const lock = await run(`SELECT U&"\\0070g_advisory_lock"(${LOCK})`);
    const held = dbExec("postgres", `SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = ${LOCK}`);
    if (!lock.error || held !== "0") {
      problems.push(`QA3-MCP-01: U&"\\0070g_advisory_lock"(${LOCK}) ran on a read-only connection (${lock.text.split("\n")[0]}); advisory locks held afterwards: ${held}`);
    }
    const file = await run(`SELECT U&"\\0070g_read_file"('PG_VERSION') AS v`);
    if (!file.error) problems.push(`QA3-MCP-01: U&"\\0070g_read_file" read a server file: ${JSON.stringify(file.text)}`);
    const escaped = await run(`SELECT U&"!0070g_ls_dir" UESCAPE '!' ('.')`);
    if (!escaped.error) problems.push(`QA3-MCP-01: U&"!0070g_ls_dir" UESCAPE '!' listed the data directory`);

    // QA3-MCP-02: statistics resets.
    for (const sql of ["SELECT pg_stat_reset()", "SELECT pg_stat_reset_shared('bgwriter')"]) {
      const r = await run(sql);
      if (!r.error) problems.push(`QA3-MCP-02: ${sql} ran on a read-only connection`);
    }
  } finally {
    mcp.kill();
  }
  assert.deepEqual(problems, []);
}
