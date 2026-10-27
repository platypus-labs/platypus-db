// BUG SAFE-10: the "Change every row?" confirmation names the wrong table on SQL Server
// statements with TOP or an alias, and cannot count its rows:
//  - "DELETE TOP (2) FROM qa_safe_g"  -> "changes every row of TOP (row count unknown)"
//    (and it changes at most 2 rows, not every row);
//  - "UPDATE g SET v = 'x' FROM qa_safe_g g JOIN qa_safe_h h ON 1 = 1" -> "every row of g".
// MySQL does the same with "DELETE g FROM qa_safe_g g JOIN qa_safe_h h" ("every row of g").
// platypus-mcp's refusal uses the same analysis, so agents read "every row of `TOP`".
// Steps: SQL Server editor (sa / Platypus-2026 from docker/compose.yml), run each statement.
// Expected: the real table qa_safe_g with "about 3 rows" (or, for TOP, no claim of every row).
// Actual: "TOP" and "g", row count unknown. Skipped when SQL Server is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { connectionToml, dbExec, needs, openEditor, runOrGuard } from "../qa-safe-db.mjs";

export const skip = needs("sqlserver");

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${connectionToml("sqlserver", "00000000-0000-4000-8000-000000000483", "QA MSSQL")}`);
  dbExec("sqlserver", "DROP TABLE IF EXISTS qa_safe_g, qa_safe_h; CREATE TABLE qa_safe_g (id int primary key, v nvarchar(10)); CREATE TABLE qa_safe_h (id int); INSERT INTO qa_safe_g VALUES (1,'a'),(2,'b'),(3,'c'); INSERT INTO qa_safe_h VALUES (1)");
}

export default async function (page) {
  try {
    await openEditor(page, "QA MSSQL");
    const top = await runOrGuard(page, "DELETE TOP (2) FROM qa_safe_g");
    const alias = await runOrGuard(page, "UPDATE g SET v = 'x' FROM qa_safe_g g JOIN qa_safe_h h ON 1 = 1");
    assert.equal(dbExec("sqlserver", "SELECT count(*) FROM qa_safe_g WHERE v <> 'x'"), "3");
    assert.doesNotMatch(top, /every row of TOP/, top);
    assert.match(alias, /every row of (dbo\.)?qa_safe_g/, alias);
  } finally {
    dbExec("sqlserver", "DROP TABLE IF EXISTS qa_safe_g, qa_safe_h");
  }
}
