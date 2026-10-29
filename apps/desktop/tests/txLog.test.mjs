// Unit tests of the session's transaction log (src/guards/txLog.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { emptyLog, groups, LOG_LIMIT, noteEnd, noteStatement } from "../src/guards/txLog.ts";

const run = (log, sql, kind, openAfter, at = 1, failed = false) => noteStatement(log, { at, sql, kind, failed }, openAfter);

test("statements in autocommit stand alone", () => {
  let log = run(emptyLog(), "UPDATE t SET a = 1", "dml", false);
  log = run(log, "SELECT 1", "query", null);
  assert.deepEqual(
    log.entries.map((e) => e.tx),
    [null, null],
  );
  assert.equal(groups(log).length, 2);
  assert.equal(groups(log)[0].entries[0].sql, "SELECT 1", "newest first");
});

test("a transaction gathers its statements and records how it ended", () => {
  let log = run(emptyLog(), "BEGIN", "other", true, 10);
  log = run(log, "INSERT INTO t VALUES (1)", "dml", true, 11);
  log = run(log, "SELECT * FROM t", "query", null, 12);
  log = run(log, "ROLLBACK", "other", false, 13);
  log = run(log, "DELETE FROM t", "dml", true, 14);
  log = noteEnd(log, true, 15);
  const [second, first] = groups(log);
  assert.deepEqual(first.tx, { id: 1, started: 10, ended: 13, end: "rollback" });
  assert.equal(first.entries.length, 4);
  assert.deepEqual(second.tx, { id: 2, started: 14, ended: 15, end: "commit" });
  assert.equal(log.open, null);
});

test("DDL that commits by itself ends the transaction as a commit; ROLLBACK TO does not end it", () => {
  let log = run(emptyLog(), "INSERT INTO t VALUES (1)", "dml", true);
  log = run(log, "ROLLBACK TO SAVEPOINT a", "other", true);
  assert.equal(log.open, 1);
  log = run(log, "CREATE TABLE u (id int)", "ddl", false);
  assert.equal(log.txs[0].end, "commit");
});

test("a failed statement is kept, in its transaction", () => {
  let log = run(emptyLog(), "BEGIN", "other", true);
  log = run(log, "INSERT INTO nope VALUES (1)", "dml", true, 2, true);
  assert.deepEqual([log.entries[1].failed, log.entries[1].tx], [true, 1]);
});

test("the log keeps the newest statements", () => {
  let log = emptyLog();
  for (let i = 0; i < LOG_LIMIT + 5; i++) log = run(log, `SELECT ${i}`, "query", null);
  assert.equal(log.entries.length, LOG_LIMIT);
  assert.equal(log.entries[0].sql, "SELECT 5");
});
