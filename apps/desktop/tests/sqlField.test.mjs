// Unit tests of the SQL field above a result's grid (src/analysis/sqlField.ts): what its text
// runs, when the in-memory copy is loaded again and how its query's events build the output.
import assert from "node:assert/strict";
import { test } from "node:test";
import { oneLine, resultCatalog, resultQuery, settled, staleCopy, topLevel, withEvent } from "../src/analysis/sqlField.ts";

test("a whole query runs as typed, without its semicolon", () => {
  assert.equal(resultQuery("SELECT x, count(*) FROM result GROUP BY x;"), "SELECT x, count(*) FROM result GROUP BY x");
  assert.equal(resultQuery("  with t as (select 1) select * from t"), "with t as (select 1) select * from t");
  assert.equal(resultQuery("FROM r2 WHERE id > 3"), "FROM r2 WHERE id > 3");
  assert.equal(resultQuery("(SELECT 1) UNION (SELECT 2)"), "(SELECT 1) UNION (SELECT 2)");
  assert.equal(resultQuery("summarize result"), "summarize result");
});

test("no text runs nothing", () => {
  assert.equal(resultQuery(""), null);
  assert.equal(resultQuery("  ; "), null);
});

test("a clause applies to the result", () => {
  assert.equal(resultQuery("WHERE code LIKE 'B%'"), "SELECT * FROM result WHERE code LIKE 'B%'");
  assert.equal(resultQuery("order by label_ita desc"), "SELECT * FROM result order by label_ita desc");
  assert.equal(resultQuery("LIMIT 10"), "SELECT * FROM result LIMIT 10");
  assert.equal(resultQuery("JOIN r2 USING (id)"), "SELECT * FROM result JOIN r2 USING (id)");
  assert.equal(resultQuery("left outer join r2 on r2.id = result.id"), "SELECT * FROM result left outer join r2 on r2.id = result.id");
});

test("anything else is the condition of a WHERE", () => {
  assert.equal(resultQuery("code = 'X'"), "SELECT * FROM result WHERE code = 'X'");
  assert.equal(resultQuery("n > 2 ORDER BY n"), "SELECT * FROM result WHERE n > 2 ORDER BY n");
  // Words that only start a clause name: a column called `wherever` is a condition.
  assert.equal(resultQuery("wherever IS NULL"), "SELECT * FROM result WHERE wherever IS NULL");
});

test("a GROUP BY selects its expressions and the count of each group", () => {
  assert.equal(resultQuery("GROUP BY code"), "SELECT code, count(*) AS count FROM result GROUP BY code");
  assert.equal(
    resultQuery("WHERE n > 1 GROUP BY a, lower(b) ORDER BY count DESC"),
    "SELECT a, lower(b), count(*) AS count FROM result WHERE n > 1 GROUP BY a, lower(b) ORDER BY count DESC",
  );
  assert.equal(resultQuery("x = 'group by y' GROUP BY x HAVING count(*) > 1"), "SELECT x, count(*) AS count FROM result WHERE x = 'group by y' GROUP BY x HAVING count(*) > 1");
  assert.equal(resultQuery("GROUP BY ALL"), "SELECT *, count(*) AS count FROM result GROUP BY ALL");
});

test("keywords in strings, quoted names and parentheses are not the statement's", () => {
  const sql = `a = 'order by' AND "group by" IN (SELECT 1 ORDER BY 1) ORDER BY a`;
  const masked = topLevel(sql);
  assert.equal(masked.length, sql.length);
  assert.equal(masked.match(/order by/gi)?.length, 1);
  assert.ok(!/group by/i.test(masked));
  // A quote doubled inside a string keeps it open.
  assert.ok(!/limit/i.test(topLevel("'it''s limit' ")));
});

test("the copy is loaded again only when the tab's results changed", () => {
  const copy = [
    { job_id: 4, table: "result" },
    { job_id: 4, table: "r1" },
    { job_id: 9, table: "r2" },
  ];
  assert.equal(staleCopy(copy, [...copy].reverse()), false);
  assert.equal(staleCopy(copy, [{ job_id: 9, table: "result" }, ...copy.slice(1)]), true);
  assert.equal(staleCopy(copy, copy.slice(0, 2)), true);
});

test("the catalog completes the copy's tables and columns", () => {
  const col = (name) => ({ name, db_type: "INTEGER", numeric: true, binary: false });
  assert.deepEqual(resultCatalog([{ table: "result", columns: [col("id"), col("n")] }, { table: "r2", columns: [col("x")] }]), {
    default_schema: "main",
    schemas: { main: { result: ["id", "n"], r2: ["x"] } },
    foreign_keys: [],
  });
});

test("the output builds from its query's events and shows once it has started", () => {
  let r = { id: -1, sql: "SELECT 1", offset: 0, jobId: null, columns: null, rows: 0, fetching: false, status: { kind: "running" }, limit: null };
  const columns = [{ name: "x", db_type: "INTEGER", numeric: true, binary: false }];
  r = withEvent(r, { type: "started", columns, elapsed_ms: 3 });
  assert.deepEqual(r.columns, columns);
  // The job may come after the first events: without it the grid cannot read the rows.
  assert.equal(settled(r), false);
  r = { ...r, jobId: 12 };
  assert.equal(settled(r), true);
  r = withEvent(r, { type: "progress", rows: 40, fetching: true });
  assert.equal(r.rows, 40);
  r = withEvent(r, { type: "finished", rows: 41, elapsed_ms: 5, complete: true, stopped: false });
  assert.deepEqual(r.status, { kind: "rows", finished: true, complete: true, stopped: false, elapsedMs: 5 });
  assert.equal(r.fetching, false);
  const error = { kind: "other", message: "no such column", code: null, detail: null, hint: null, position: null };
  assert.deepEqual(withEvent(r, { type: "failed", error }).status, { kind: "error", error });
});

test("a query from the larger editor shows on one line", () => {
  assert.equal(oneLine("SELECT a\n  FROM result\nWHERE a > 1"), "SELECT a FROM result WHERE a > 1");
});
