// BUG SAFE-08 (low): the "Change every row?" confirmation names the statement by its first
// word, not by what it does: "WITH without WHERE changes every row of t" for a DELETE after a
// CTE. (TRUNCATE also reads "TRUNCATE without WHERE", though TRUNCATE has no WHERE.) A
// statement that starts with a comment is labelled right: the comment is not part of it.
// GuardDialog.tsx: `preview(u.sql).split(" ")[0]?.toUpperCase()`.
// Steps: SQLite editor, run "WITH x AS (SELECT 1) DELETE FROM t", then
// "/* tidy */ DELETE FROM t".
// Expected: "DELETE without WHERE changes every row of t" both times.
// Actual: "WITH without WHERE changes every row of t" for the first.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { openEditor, runOrGuard } from "../qa-safe-db.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "g.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1), (2), (3);");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000481"
name = "Guard db"
driver = "sqlite"
database = '${join(dir, "g.db")}'
save_password = false
`,
  );
}

export default async function (page) {
  await openEditor(page, "Guard db");
  const results = [];
  for (const sql of ["WITH x AS (SELECT 1) DELETE FROM t", "/* tidy */ DELETE FROM t"]) {
    const r = await runOrGuard(page, sql);
    assert.match(r, /^guard: /, `no confirmation for ${sql}: ${r}`);
    results.push(r);
  }
  const wrong = results.filter((r) => !/DELETE without WHERE changes every row of t/.test(r));
  assert.deepEqual(wrong, [], wrong.join("\n"));
}

