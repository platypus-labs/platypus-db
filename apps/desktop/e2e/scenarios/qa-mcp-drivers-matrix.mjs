// Group S: the MCP server on the drivers qa3-mcp-readonly-matrix does not reach: SQL Server,
// ClickHouse, Redis and Oracle (Oracle only when an Instant Client is at hand). On each, with
// the app open beside it:
// - a connection whose agent access is "read_only" lists as such, refuses writes however they
//   are written (DML, DDL, TRUNCATE, commands of Redis that change keys) with "read-only for
//   agents", runs reads, and the database is unchanged afterwards;
// - a connection of "read_write" with "Ask before agents change data" on waits for the user in
//   the app: the question shows the statement; refused (the agent reads the reason, nothing
//   changed), allowed once (the change is made; the next one asks again) and allowed for the
//   session (the next runs unasked); a read never asks.
// Cassandra and Elasticsearch have containers only under compose profiles (cassandra, search)
// that are not started with the others: their part runs when the port answers and is skipped,
// with the reason, otherwise.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";
import { noMcp, startMcp } from "../mcp.mjs";
import { setField } from "../plans.mjs";
import { needs as needsDb, sh } from "../qa-grid.mjs";
import { clientRoot, installClient, ora, oracleToml } from "../qa-ora.mjs";
import { connection as redisConnection, redis, skip as redisSkip, text } from "../redis-helpers.mjs";

const dialog = `document.querySelector('.agent-write')`;
const field = (name) => `(${dialog}?.querySelector('[data-field=${JSON.stringify(name)}]')?.textContent ?? '')`;
const answer = (verdict) => `${dialog}.querySelector('[data-answer=${JSON.stringify(verdict)}]').click()`;

const id = (n) => `00000000-0000-4000-8000-0000000d${String(n).padStart(4, "0")}`;
const WRITE_ACCESS = 'agent_access = "read_write"\n';

const reachable = (port) =>
  new Promise((resolve) => {
    const socket = tcp({ host: "127.0.0.1", port, timeout: 1000 });
    socket.on("connect", () => (socket.destroy(), resolve(true)));
    socket.on("error", () => resolve(false));
    socket.on("timeout", () => (socket.destroy(), resolve(false)));
  });

/** One driver: its saved connections (`toml(id, name, extra)`), what it holds (`seed`), a
 *  snapshot of that (`state`), what read-only refuses, a read that works, and the changes the
 *  user is asked about. */
const DRIVERS = [
  {
    name: "SQL Server",
    skip: () => needsDb("mssql")(),
    toml: (n, name, extra) => `[[connections]]\nid = "${id(n)}"\nname = "${name}"\ndriver = "sqlserver"\nhost = "localhost"\nport = 51433\ndatabase = "platypus"\nuser = "sa"\nssl_mode = "disable"\nsave_password = false\npassword_command = "echo Platypus-2026"\n${extra}`,
    seed: () =>
      sh(
        "mssql",
        "IF OBJECT_ID('dbo.qa_mcp_t') IS NOT NULL DROP TABLE dbo.qa_mcp_t; CREATE TABLE dbo.qa_mcp_t (id int PRIMARY KEY, v varchar(10)); INSERT INTO dbo.qa_mcp_t VALUES (1, 'a'), (2, 'b'), (3, 'c'), (4, 'd');",
      ),
    cleanup: () => sh("mssql", "IF OBJECT_ID('dbo.qa_mcp_t') IS NOT NULL DROP TABLE dbo.qa_mcp_t;"),
    state: () => sh("mssql", "SET NOCOUNT ON; SELECT COALESCE(STRING_AGG(CAST(id AS varchar(5)) + v, ',') WITHIN GROUP (ORDER BY id), '') FROM dbo.qa_mcp_t"),
    refused: [
      "INSERT INTO dbo.qa_mcp_t VALUES (9, 'z')",
      "UPDATE dbo.qa_mcp_t SET v = 'x' WHERE id = 1",
      "DELETE FROM dbo.qa_mcp_t WHERE id = 1",
      "TRUNCATE TABLE dbo.qa_mcp_t",
      "DROP TABLE dbo.qa_mcp_t",
      "ALTER TABLE dbo.qa_mcp_t ADD w int",
      "MERGE dbo.qa_mcp_t AS t USING (SELECT 1 AS id) AS s ON t.id = s.id WHEN MATCHED THEN DELETE;",
      "/* hidden */ DELETE FROM dbo.qa_mcp_t WHERE id = 2",
      "EXEC sp_executesql N'DELETE FROM dbo.qa_mcp_t WHERE id = 2'",
    ],
    read: ["SELECT count(*) AS n FROM dbo.qa_mcp_t", /\n4\n?/],
    initial: "1a,2b,3c,4d",
    deny: "DELETE FROM dbo.qa_mcp_t WHERE id = 4",
    once: ["UPDATE dbo.qa_mcp_t SET v = 'q' WHERE id = 1", "1q,2b,3c,4d"],
    session: ["DELETE FROM dbo.qa_mcp_t WHERE id = 3", "1q,2b,4d"],
    unasked: ["DELETE FROM dbo.qa_mcp_t WHERE id = 2", "1q,4d"],
  },
  {
    name: "ClickHouse",
    skip: () => needsDb("clickhouse")(),
    toml: (n, name, extra) => `[[connections]]\nid = "${id(n)}"\nname = "${name}"\ndriver = "clickhouse"\nhost = "localhost"\nport = 58123\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"\nsave_password = false\npassword_command = "echo platypus"\n${extra}`,
    seed: () =>
      sh(
        "clickhouse",
        "DROP TABLE IF EXISTS qa_mcp_t; CREATE TABLE qa_mcp_t (id Int32, v String) ENGINE = MergeTree ORDER BY id; INSERT INTO qa_mcp_t VALUES (1, 'a'), (2, 'b'), (3, 'c'), (4, 'd');",
      ),
    cleanup: () => sh("clickhouse", "DROP TABLE IF EXISTS qa_mcp_t;"),
    state: () => sh("clickhouse", "SELECT arrayStringConcat(arraySort(groupArray(concat(toString(id), v))), ',') FROM qa_mcp_t"),
    refused: [
      "INSERT INTO qa_mcp_t VALUES (9, 'z')",
      "ALTER TABLE qa_mcp_t DELETE WHERE id = 1",
      "ALTER TABLE qa_mcp_t UPDATE v = 'x' WHERE id = 1",
      "TRUNCATE TABLE qa_mcp_t",
      "DROP TABLE qa_mcp_t",
      "OPTIMIZE TABLE qa_mcp_t FINAL",
      "CREATE TABLE qa_mcp_new (a Int32) ENGINE = Memory",
      "RENAME TABLE qa_mcp_t TO qa_mcp_u",
      "KILL QUERY WHERE query_id = 'none'",
      "SYSTEM FLUSH LOGS",
      "SET max_threads = 1",
      "/* hidden */ INSERT INTO qa_mcp_t VALUES (8, 'y')",
    ],
    read: ["SELECT count() AS n FROM qa_mcp_t", /\n4\n?/],
    initial: "1a,2b,3c,4d",
    deny: "ALTER TABLE qa_mcp_t DELETE WHERE id = 4",
    once: ["INSERT INTO qa_mcp_t VALUES (5, 'e')", "1a,2b,3c,4d,5e"],
    session: ["INSERT INTO qa_mcp_t VALUES (6, 'f')", "1a,2b,3c,4d,5e,6f"],
    unasked: ["INSERT INTO qa_mcp_t VALUES (7, 'g')", "1a,2b,3c,4d,5e,6f,7g"],
  },
  {
    name: "Redis",
    skip: redisSkip,
    toml: (n, name, extra) => redisConnection(n, name, 0, extra).replace(/id = "[^"]+"/, `id = "${id(n)}"`),
    seed: () => redis(0, [["DEL", "qa:mcp:k", "qa:mcp:l", "qa:mcp:w1", "qa:mcp:w2", "qa:mcp:w3"], ["SET", "qa:mcp:k", "v1"], ["RPUSH", "qa:mcp:l", "a", "b"]]),
    cleanup: () => redis(0, [["DEL", "qa:mcp:k", "qa:mcp:l", "qa:mcp:w1", "qa:mcp:w2", "qa:mcp:w3"]]),
    state: async () => {
      const [k, l, w1, w2, w3] = await redis(0, [["GET", "qa:mcp:k"], ["LRANGE", "qa:mcp:l", 0, -1], ["GET", "qa:mcp:w1"], ["GET", "qa:mcp:w2"], ["GET", "qa:mcp:w3"]]);
      return JSON.stringify([text(k), (l ?? []).map(text), text(w1), text(w2), text(w3)]);
    },
    refused: [
      "SET qa:mcp:k changed",
      "DEL qa:mcp:k",
      "EXPIRE qa:mcp:k 1",
      "LPUSH qa:mcp:l z",
      "RPOP qa:mcp:l",
      "INCR qa:mcp:counter",
      "RENAME qa:mcp:k qa:mcp:k2",
      "FLUSHDB",
      "FLUSHALL",
      "CONFIG SET maxmemory 1mb",
      "set qa:mcp:k lower-case",
      "EVAL \"return redis.call('DEL','qa:mcp:k')\" 0",
    ],
    read: ["GET qa:mcp:k", /v1/],
    initial: JSON.stringify(["v1", ["a", "b"], null, null, null]),
    deny: "DEL qa:mcp:k",
    once: ["SET qa:mcp:w1 one", JSON.stringify(["v1", ["a", "b"], "one", null, null])],
    session: ["SET qa:mcp:w2 two", JSON.stringify(["v1", ["a", "b"], "one", "two", null])],
    unasked: ["SET qa:mcp:w3 three", JSON.stringify(["v1", ["a", "b"], "one", "two", "three"])],
  },
  {
    name: "Oracle",
    skip: async () => {
      if (!(await reachable(51521))) return "no Oracle on port 51521 (docker compose -f docker/compose.yml --profile oracle up -d)";
      return clientRoot() ? null : "no Instant Client (set PLATYPUS_E2E_ORACLE_DRIVERS)";
    },
    toml: (n, name, extra) => {
      const access = /read_write/.test(extra) ? { agent_access: "read_write" } : {};
      return oracleToml(id(n), name, access);
    },
    seed: () => {
      ora("BEGIN EXECUTE IMMEDIATE 'DROP TABLE qa_mcp_t'; EXCEPTION WHEN OTHERS THEN NULL; END;\n/", { check: false });
      ora("CREATE TABLE qa_mcp_t (id NUMBER PRIMARY KEY, v VARCHAR2(10));\nINSERT INTO qa_mcp_t VALUES (1, 'a');\nINSERT INTO qa_mcp_t VALUES (2, 'b');\nINSERT INTO qa_mcp_t VALUES (3, 'c');\nINSERT INTO qa_mcp_t VALUES (4, 'd');\nCOMMIT;");
    },
    cleanup: () => ora("BEGIN EXECUTE IMMEDIATE 'DROP TABLE qa_mcp_t'; EXCEPTION WHEN OTHERS THEN NULL; END;\n/", { check: false }),
    state: () => ora("SELECT LISTAGG(id || v, ',') WITHIN GROUP (ORDER BY id) FROM qa_mcp_t;"),
    refused: [
      "INSERT INTO qa_mcp_t VALUES (9, 'z')",
      "UPDATE qa_mcp_t SET v = 'x' WHERE id = 1",
      "DELETE FROM qa_mcp_t WHERE id = 1",
      "TRUNCATE TABLE qa_mcp_t",
      "DROP TABLE qa_mcp_t",
      "ALTER TABLE qa_mcp_t ADD (w NUMBER)",
      "MERGE INTO qa_mcp_t t USING (SELECT 1 AS id FROM dual) s ON (t.id = s.id) WHEN MATCHED THEN DELETE",
      "BEGIN DELETE FROM qa_mcp_t WHERE id = 1; END;",
      "SELECT 1 FROM dual FOR UPDATE",
      "/* hidden */ DELETE FROM qa_mcp_t WHERE id = 2",
      "GRANT SELECT ON qa_mcp_t TO PUBLIC",
    ],
    read: ["SELECT count(*) AS n FROM qa_mcp_t", /\n4\n?/],
    initial: "1a,2b,3c,4d",
    deny: "DELETE FROM qa_mcp_t WHERE id = 4",
    once: ["UPDATE qa_mcp_t SET v = 'q' WHERE id = 1", "1q,2b,3c,4d"],
    session: ["DELETE FROM qa_mcp_t WHERE id = 3", "1q,2b,4d"],
    unasked: ["DELETE FROM qa_mcp_t WHERE id = 2", "1q,4d"],
  },
];

/** The drivers whose server answers, with the others' reasons. */
const active = [];
const skipped = [];

export async function skip() {
  const missing = noMcp();
  if (missing) return missing;
  for (const d of DRIVERS) {
    const why = await d.skip();
    (why ? skipped : active).push(why ? [d.name, why] : d);
  }
  if (active.length === 0) return `no database to try: ${skipped.map(([n, w]) => `${n}: ${w}`).join("; ")}`;
  return null;
}

export function setup({ dir }) {
  let text_ = "version = 7\n";
  DRIVERS.forEach((d, i) => {
    text_ += `${d.toml(i * 2 + 1, `QA ${d.name} ro`, "")}\n${d.toml(i * 2 + 2, `QA ${d.name} rw`, WRITE_ACCESS)}\n`;
  });
  writeFileSync(join(dir, "connections.toml"), text_);
  // The questions of the app, which the runner turns off by default.
  writeFileSync(join(dir, "settings.toml"), "[agents]\nconfirm_writes = true\n");
  if (clientRoot()) installClient(dir);
}

/** Runs `sql` through the agent and waits for the app's question about it. */
async function asked(page, mcp, connection, sql) {
  const result = mcp.call("run_query", { connection, sql });
  await page.waitFor(`${dialog} !== null && ${field("statement")} === ${JSON.stringify(sql)}`, 30_000);
  return [result];
}

async function readOnly(mcp, d, name) {
  const list = JSON.parse((await mcp.call("list_connections")).text);
  const entry = list.find((c) => c.name === name);
  assert.ok(entry, `${name} is listed`);
  assert.equal(entry.agent_access, "read_only");
  for (const sql of d.refused) {
    const r = await mcp.call("run_query", { connection: name, sql });
    assert.ok(r.error, `${d.name} read-only ran: ${sql}: ${r.text}`);
    assert.match(r.text, /read-only|read only|refus|not allowed|cannot|may not/i, `${sql}: ${r.text}`);
  }
  assert.equal(await d.state(), d.initial, `${d.name}: read-only left the data as it was`);
  const [sql, match] = d.read;
  const r = await mcp.call("run_query", { connection: name, sql });
  assert.equal(r.error, false, `${d.name}: ${sql}: ${r.text}`);
  assert.match(r.text, match, r.text);
  // The tools that read work too; one that would write does not exist.
  const objects = await mcp.call("list_schemas", { connection: name });
  assert.equal(objects.error, false, objects.text);
}

async function withQuestions(page, mcp, d, name) {
  const list = JSON.parse((await mcp.call("list_connections")).text);
  assert.equal(list.find((c) => c.name === name).agent_access, "read_write");

  // A read never asks.
  const [readSql, match] = d.read;
  const read = await mcp.call("run_query", { connection: name, sql: readSql });
  assert.equal(read.error, false, read.text);
  assert.match(read.text, match);
  assert.equal(await page.eval(`${dialog} === null`), true, "a read asked");

  // Refused, with a reason: the agent reads it and nothing changed.
  let [result] = await asked(page, mcp, name, d.deny);
  assert.match(await page.eval(`${dialog}.querySelector('h2').textContent`), new RegExp(`wants to change data on ${name}`));
  await setField(page, ".agent-write [data-field=reason]", "not before the release");
  await page.eval(answer("deny"));
  let r = await result;
  assert.ok(r.error && /refused this change: not before the release/.test(r.text), r.text);
  assert.equal(await d.state(), d.initial, `${d.name}: a refused change ran`);
  await page.waitFor(`${dialog} === null`);

  // Allowed once: it runs, and the next one asks again.
  [result] = await asked(page, mcp, name, d.once[0]);
  await page.eval(answer("once"));
  r = await result;
  assert.equal(r.error, false, r.text);
  assert.equal(await d.state(), d.once[1], `${d.name}: the change allowed once`);
  [result] = await asked(page, mcp, name, d.session[0]);
  assert.equal(await page.eval(`${dialog}.querySelector('[data-field=reason]').value`), "");
  // Allowed for the session: the next change of this agent here runs unasked.
  await page.eval(answer("session"));
  assert.equal((await result).error, false);
  assert.equal(await d.state(), d.session[1]);
  r = await mcp.call("run_query", { connection: name, sql: d.unasked[0] });
  assert.equal(r.error, false, r.text);
  assert.equal(await page.eval(`${dialog} === null`), true, "asked again after the session was allowed");
  assert.equal(await d.state(), d.unasked[1]);
}

export default async function (page, { dir }) {
  for (const [name, why] of skipped) console.log(`     skip ${name}: ${why}`);
  const mcp = await startMcp(dir, "qa-mcp-matrix");
  try {
    for (const d of active) {
      await d.seed();
      try {
        assert.equal(await d.state(), d.initial, `${d.name}: seeded`);
        await readOnly(mcp, d, `QA ${d.name} ro`);
        await withQuestions(page, mcp, d, `QA ${d.name} rw`);
      } catch (e) {
        e.message = `[${d.name}] ${e.message}`;
        throw e;
      } finally {
        await d.cleanup();
      }
    }
  } finally {
    mcp.close();
  }
}
