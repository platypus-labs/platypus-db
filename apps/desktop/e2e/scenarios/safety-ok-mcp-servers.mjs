// Regression guard (passes): platypus-mcp on PostgreSQL, MySQL and MongoDB from
// docker/compose.yml. list_connections shows no host or user; read-only connections refuse
// DDL and DML (and pg_terminate_backend), read_write ones run them and still refuse a DELETE
// without WHERE; a timeout cancels the statement and the next call works; a wrong password's
// error never shows the password; the row limit
// says the result has more. Skipped when PostgreSQL is not running (MySQL and MongoDB parts
// run when they are up).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { connectionToml, dbExec, needs } from "../safety-helpers.mjs";
import { noMcp, startMcp } from "../mcp.mjs";

const WRONG = "QaWrongPassword-4410";

export async function skip() {
  return noMcp() ?? (await needs("postgres")());
}

export function setup({ dir }) {
  const id = (n) => `00000000-0000-4000-8000-00000000051${n}`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n` +
      connectionToml("postgres", id(1), "PG ro") +
      connectionToml("postgres", id(2), "PG rw", 'agent_access = "read_write"') +
      connectionToml("postgres", id(3), "PG wrong").replace("echo platypus", `echo ${WRONG}`) +
      connectionToml("mysql", id(4), "MY ro") +
      connectionToml("mysql", id(5), "MY rw", 'agent_access = "read_write"') +
      connectionToml("mongo", id(6), "Mongo ro"),
  );
}

export default async function (_page, { dir }) {
  const mysql = !(await needs("mysql")());
  const mongo = !(await needs("mongo")());
  const mcp = await startMcp(dir);
  const run = (connection, sql, extra = {}) => mcp.call("run_query", { connection, sql, ...extra });
  try {
    const list = (await mcp.call("list_connections")).text;
    assert.doesNotMatch(list, /localhost|127\.0\.0\.1|"user"|"host"|password/i, list);

    assert.match((await run("PG ro", "CREATE TABLE qa_safe_m (id int)")).text, /read-only for agents/);
    assert.match((await run("PG ro", "SELECT pg_terminate_backend(pg_backend_pid())")).text, /read-only for agents/);
    assert.equal((await run("PG rw", "CREATE TABLE qa_safe_m (id int primary key)")).error, false);
    assert.equal((await run("PG rw", "INSERT INTO qa_safe_m VALUES (1), (2)")).error, false);
    assert.match((await run("PG ro", "UPDATE qa_safe_m SET id = 3 WHERE id = 1")).text, /read-only for agents/);
    assert.match((await run("PG rw", "DELETE FROM qa_safe_m")).text, /every row of `qa_safe_m`/);
    assert.equal(dbExec("postgres", "SELECT count(*) FROM qa_safe_m"), "2");
    assert.match((await run("PG ro", "SELECT g FROM generate_series(1, 200) g")).text, /100 rows shown.*has more/s);

    const started = Date.now();
    const slow = await run("PG ro", "SELECT pg_sleep(20)", { timeout_seconds: 1 });
    assert.match(slow.text, /cancelled after 1s/, slow.text);
    assert.ok(Date.now() - started < 15_000, "the timeout did not cut the statement short");
    assert.match((await run("PG ro", "SELECT 'alive' AS s")).text, /alive/);

    const wrong = await run("PG wrong", "SELECT 1");
    assert.ok(wrong.error && /password authentication failed/.test(wrong.text), wrong.text);
    assert.ok(!wrong.text.includes(WRONG));

    if (mysql) {
      assert.match((await run("MY ro", "CREATE TABLE qa_safe_m (id int)")).text, /read-only for agents/);
      assert.equal((await run("MY rw", "CREATE TABLE qa_safe_m (id int primary key)")).error, false);
      assert.match((await run("MY ro", "INSERT INTO qa_safe_m VALUES (1)")).text, /read-only for agents/);
      assert.match((await run("MY rw", "DELETE FROM qa_safe_m")).text, /every row/);
      assert.equal((await run("MY rw", "DROP TABLE qa_safe_m")).error, false);
    }
    if (mongo) {
      assert.match((await run("Mongo ro", "db.qa_safe_c.insertOne({ a: 1 })")).text, /read-only for agents/);
      assert.match((await run("Mongo ro", "db.customers.aggregate([{ $out: 'qa_safe_out' }])")).text, /read-only for agents/);
      assert.equal((await run("Mongo ro", "db.customers.find({}).limit(1)")).error, false);
    }
  } finally {
    mcp.close();
    dbExec("postgres", "DROP TABLE IF EXISTS qa_safe_m");
    if (mysql) dbExec("mysql", "DROP TABLE IF EXISTS qa_safe_m");
  }
}
