// QA3 MCP read-only enforcement (regression guard, passes) on SQLite, MySQL and SQL Server,
// the app open beside: statements that write, change the session, reach files or other
// sessions are refused, however they are dressed (comments, CTE, executable comments,
// quoting, batches, GO); reads run; the databases are unchanged afterwards. A connection
// marked read-only in the app, or production, stays read-only for agents even when its
// agent access says read_write. MySQL and SQL Server parts run when their containers are up.
import assert from "node:assert/strict";
import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { connectionToml, dbExec, needs } from "../safety-helpers.mjs";
import { noMcp, sqliteToml, startClient } from "../mcp-client.mjs";

export const skip = noMcp;

const id = (n) => `00000000-0000-4000-8000-0000000003${30 + n}`;

export function setup({ dir }) {
  const file = join(dir, "matrix.db");
  const db = new DatabaseSync(file);
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a'), (2, 'b'); PRAGMA user_version = 7;");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    "version = 7\n" +
      sqliteToml(id(1), "QA3 lite ro", file) +
      sqliteToml(id(2), "QA3 lite app-ro", file, 'read_only = true\nagent_access = "read_write"') +
      sqliteToml(id(3), "QA3 lite prod", file, 'connection_type = "prod"\nagent_access = "read_write"') +
      connectionToml("mysql", id(4), "QA3 my ro") +
      connectionToml("sqlserver", id(5), "QA3 ms ro"),
  );
}

const fwd = (p) => p.replaceAll("\\", "/");

const sqliteRefused = (dir) => [
  "DELETE FROM t WHERE id = 1",
  "WITH x AS (SELECT 1) DELETE FROM t WHERE id = 1",
  "/* ; */ UPDATE t SET v = 'z' WHERE id = 2",
  "SELECT 1; DELETE FROM t",
  "PRAGMA user_version = 99",
  "PRAGMA query_only = OFF",
  "PRAGMA journal_mode = DELETE",
  `ATTACH '${fwd(join(dir, "evil.db"))}' AS evil`,
  `VACUUM INTO '${fwd(join(dir, "copy.db"))}'`,
  "SELECT load_extension('evil')",
  "SELECT [load_extension]('evil')",
  "REINDEX",
  "SAVEPOINT s",
  "CREATE TEMP TABLE x (a)",
];

async function sqlite(mcp, dir) {
  for (const conn of ["QA3 lite ro", "QA3 lite app-ro", "QA3 lite prod"]) {
    const list = JSON.parse((await mcp.call("list_connections")).text);
    assert.equal(list.find((c) => c.name === conn).agent_access, "read_only", `${conn} is read-only for agents`);
    for (const sql of sqliteRefused(dir)) {
      const r = await mcp.call("run_query", { connection: conn, sql });
      assert.ok(r.error, `${conn} ran: ${sql}: ${r.text}`);
    }
    assert.match((await mcp.call("run_query", { connection: conn, sql: "SELECT count(*) AS n FROM t" })).text, /\n2\n/);
  }
  const db = new DatabaseSync(join(dir, "matrix.db"));
  assert.equal(db.prepare("SELECT count(*) AS n FROM t").get().n, 2);
  assert.equal(db.prepare("PRAGMA user_version").get().user_version, 7);
  assert.equal(db.prepare("PRAGMA journal_mode").get().journal_mode, "delete");
  db.close();
  assert.ok(!existsSync(join(dir, "evil.db")) && !existsSync(join(dir, "copy.db")), "no file was written");
}

const MYSQL_REFUSED = [
  "DELETE FROM qa3_my WHERE id = 1",
  "SELECT 1 INTO OUTFILE '/tmp/qa3_out'",
  "SELECT 1 /*!50000 INTO OUTFILE '/tmp/qa3_out2' */",
  "SELECT /*!80000 LOAD_FILE('/etc/hostname') */",
  "SELECT `load_file`('/etc/hostname')",
  "SELECT GET_LOCK('qa3', 1)",
  "LOCK TABLES qa3_my READ",
  "OPTIMIZE TABLE qa3_my",
  "FLUSH TABLES",
  "KILL 999999",
  "SET GLOBAL max_connections = 1",
  "SELECT * FROM qa3_my FOR UPDATE; DELETE FROM qa3_my",
  "XA START 'qa3'",
  "DO SLEEP(0)",
];

async function mysql(mcp) {
  dbExec("mysql", "DROP TABLE IF EXISTS qa3_my; CREATE TABLE qa3_my (id int PRIMARY KEY); INSERT INTO qa3_my VALUES (1), (2);");
  for (const sql of MYSQL_REFUSED) {
    const r = await mcp.call("run_query", { connection: "QA3 my ro", sql });
    assert.ok(r.error, `MySQL read-only ran: ${sql}: ${r.text}`);
  }
  assert.match((await mcp.call("run_query", { connection: "QA3 my ro", sql: "SELECT count(*) AS n FROM qa3_my" })).text, /\n2\n/);
  assert.equal(dbExec("mysql", "SELECT count(*) FROM qa3_my"), "2");
}

const SQLSERVER_REFUSED = [
  "DELETE FROM dbo.qa3_ms WHERE id = 1",
  "SELECT NEXT VALUE FOR dbo.qa3_ms_seq AS n",
  "SELECT 1 AS a INTO dbo.qa3_ms_new",
  "SELECT 1 AS a INTO #qa3_tmp",
  "SELECT * FROM OPENROWSET(BULK '/etc/hostname', SINGLE_CLOB) AS x",
  "SELECT * FROM OPENQUERY(x, 'DELETE FROM dbo.qa3_ms')",
  "EXEC xp_cmdshell 'whoami'",
  "USE master",
  "SHUTDOWN WITH NOWAIT",
  "KILL 999",
  "DBCC FREEPROCCACHE",
  "WAITFOR DELAY '00:00:01'",
  "BACKUP DATABASE platypus TO DISK = '/tmp/qa3.bak'",
  "SELECT 1\nGO\nDELETE FROM dbo.qa3_ms",
  "BEGIN TRAN; DELETE FROM dbo.qa3_ms; COMMIT",
];

async function sqlserver(mcp) {
  dbExec(
    "sqlserver",
    "IF OBJECT_ID('dbo.qa3_ms') IS NOT NULL DROP TABLE dbo.qa3_ms; CREATE TABLE dbo.qa3_ms (id int PRIMARY KEY); INSERT INTO dbo.qa3_ms VALUES (1), (2); IF OBJECT_ID('dbo.qa3_ms_seq') IS NOT NULL DROP SEQUENCE dbo.qa3_ms_seq; CREATE SEQUENCE dbo.qa3_ms_seq START WITH 1; IF OBJECT_ID('dbo.qa3_ms_new') IS NOT NULL DROP TABLE dbo.qa3_ms_new;",
  );
  for (const sql of SQLSERVER_REFUSED) {
    const r = await mcp.call("run_query", { connection: "QA3 ms ro", sql });
    assert.ok(r.error, `SQL Server read-only ran: ${sql}: ${r.text}`);
  }
  assert.match((await mcp.call("run_query", { connection: "QA3 ms ro", sql: "SELECT count(*) AS n FROM dbo.qa3_ms" })).text, /\n2\n/);
  assert.match((await mcp.call("run_query", { connection: "QA3 ms ro", sql: "SELECT DB_NAME() AS d" })).text, /platypus/, "still on its database");
  assert.equal(dbExec("sqlserver", "SELECT count(*) FROM dbo.qa3_ms"), "2");
  assert.equal(dbExec("sqlserver", "SELECT CAST(current_value AS int) FROM sys.sequences WHERE name = 'qa3_ms_seq'"), "1");
  assert.equal(dbExec("sqlserver", "SELECT count(*) FROM sys.tables WHERE name = 'qa3_ms_new'"), "0");
}

export default async function (_page, { dir }) {
  const mcp = await startClient(dir, "qa3-readonly-matrix");
  try {
    await sqlite(mcp, dir);
    if (!(await needs("mysql")())) await mysql(mcp);
    if (!(await needs("sqlserver")())) await sqlserver(mcp);
    assert.deepEqual(mcp.notJson, []);
  } finally {
    mcp.kill();
  }
}
