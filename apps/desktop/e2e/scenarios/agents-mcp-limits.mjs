// QA3 MCP result sizes and readability, on a SQLite file with 5,000 tables, the app open.
//
// BUG QA3-MCP-06 (medium): list_objects has no limit. On a schema with 5,000 tables it
// returns every object in one ~285 KB JSON text (10,000 tables: over half a megabyte) into
// the agent's context, while search_objects stops at 200 matches with "truncated" and
// run_query at 32 KB by default. Expected: a cap (or paging) said in the result, as the
// other tools do. Suspected: crates/platypus-mcp/src/agent.rs list_objects (no cut).
//
// BUG QA3-MCP-07 (low): in run_query's text table a SQL NULL and the string 'NULL' print
// the same (`NULL | NULL`), so an agent reading the default format cannot tell them apart
// and writes `= 'NULL'` or `IS NULL` wrongly. format=json tells them apart. Expected: the
// text format marks one of them (the app's grid shows NULL in its own style).
// Suspected: crates/platypus-mcp/src/output.rs (text rendering of nulls).
//
// Passing parts (regression guards): max_rows and max_bytes are capped at 10,000 rows and
// 1 MiB and the result says it was cut; long values are cut and said so; search_objects
// stops at 200; a value holding a newline stays on its row.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { noMcp, sqliteToml, startClient } from "../mcp-client.mjs";

export const skip = noMcp;

const TABLES = 5000;

export function setup({ dir }) {
  const file = join(dir, "wide.db");
  const db = new DatabaseSync(file);
  db.exec("BEGIN");
  for (let i = 0; i < TABLES; i++) db.exec(`CREATE TABLE qa3_table_number_${i} (id INTEGER PRIMARY KEY, label TEXT)`);
  db.exec(`CREATE TABLE big (id INTEGER PRIMARY KEY, pad TEXT);
    WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 20000)
    INSERT INTO big SELECT x, printf('%0200d', x) FROM c;
    COMMIT;`);
  db.close();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${sqliteToml("00000000-0000-4000-8000-000000000341", "QA3 wide", file)}`);
}

export default async function (_page, { dir }) {
  const mcp = await startClient(dir, "qa3-limits");
  const conn = { connection: "QA3 wide" };
  const run = (sql, extra = {}) => mcp.call("run_query", { ...conn, sql, ...extra });
  const problems = [];
  try {
    // Defaults: 100 rows.
    assert.match((await run("SELECT id FROM big")).text, /100 rows shown[\s\S]*has more|100 rows shown/);
    // Caps: asking for more than 10,000 rows or 1 MiB gets the cap, said in the result.
    const capped = await run("SELECT id FROM big", { max_rows: 1_000_000, max_bytes: 100_000_000 });
    assert.match(capped.text, /10000 rows shown/, capped.text.slice(-300));
    const bytes = await run("SELECT * FROM big", { max_rows: 10_000, max_bytes: 100_000_000 });
    assert.ok(bytes.text.length <= 1_100_000, `the 1 MiB cap let ${bytes.text.length} bytes through`);
    assert.match(bytes.text, /cut at 1048576 bytes/);
    const json = JSON.parse((await run("SELECT * FROM big", { format: "json" })).text);
    assert.equal(json.truncated, true);
    // A long value is cut and said so.
    const long = await run("SELECT zeroblob(3000000) AS b");
    assert.ok(long.text.length < 50_000, `a 3 MB value took ${long.text.length} bytes`);
    assert.match(long.text, /longer than/);
    // A newline inside a value stays on its row.
    const multi = await run("SELECT 'first' || char(10) || '(0 rows, 1 ms)' AS v");
    assert.match(multi.text, /\n\(1 row, \d+ ms\)$/);
    assert.equal(multi.text.split("\n").length, 4, multi.text);

    const search = JSON.parse((await mcp.call("search_objects", { ...conn, text: "qa3_table" })).text);
    const matches = search.matches ?? search.objects ?? search.results ?? [];
    assert.equal(search.truncated, true);
    assert.ok(JSON.stringify(search).split('"name"').length - 1 <= 200 || matches.length <= 200, "search_objects stops at 200");

    // QA3-MCP-06: list_objects is bounded too.
    const objects = await mcp.call("list_objects", { ...conn, schema: "main" });
    const count = objects.text.split('"name"').length - 1;
    if (!/"truncated"\s*:\s*true/.test(objects.text) && count > 1000) {
      problems.push(`QA3-MCP-06: list_objects returned ${count} objects in ${objects.text.length} bytes, with no cap or truncation flag`);
    }

    // QA3-MCP-07: NULL and 'NULL' read differently in the text table.
    const nulls = await run("SELECT NULL AS a, 'NULL' AS b");
    const row = nulls.text.split("\n")[2] ?? "";
    const [a, b] = row.split("|").map((s) => s.trim());
    if (a === b) problems.push(`QA3-MCP-07: SQL NULL and the string 'NULL' print the same in the text table: ${JSON.stringify(row)}`);
  } finally {
    mcp.kill();
  }
  assert.deepEqual(problems, []);
}
