// BUG SAFE-09: TRUNCATE of several tables is confirmed as if it emptied only the first one.
// PostgreSQL "TRUNCATE TABLE qa_safe_t1, qa_safe_t2": the confirmation says "changes every row
// of qa_safe_t1" and counts only its rows; qa_safe_t2, emptied too, is not mentioned. The
// statement analysis keeps one `unfiltered` table per statement (platypus_sql::analyze), and
// platypus-mcp's refusal names only the first table as well.
// Steps: PG editor, run "TRUNCATE TABLE qa_safe_t1, qa_safe_t2".
// Expected: both tables listed in the confirmation.
// Actual: only qa_safe_t1. Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { connectionToml, dbExec, needs, openEditor, runOrGuard } from "../safety-helpers.mjs";

export const skip = needs("postgres");

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${connectionToml("postgres", "00000000-0000-4000-8000-000000000482", "QA PG")}`);
  dbExec("postgres", "DROP TABLE IF EXISTS qa_safe_t1, qa_safe_t2; CREATE TABLE qa_safe_t1 (id int); CREATE TABLE qa_safe_t2 (id int); INSERT INTO qa_safe_t1 VALUES (1); INSERT INTO qa_safe_t2 VALUES (1), (2)");
}

export default async function (page) {
  try {
    await openEditor(page, "QA PG");
    const r = await runOrGuard(page, "TRUNCATE TABLE qa_safe_t1, qa_safe_t2");
    assert.match(r, /^guard: /, `no confirmation: ${r}`);
    assert.equal(dbExec("postgres", "SELECT (SELECT count(*) FROM qa_safe_t1) + (SELECT count(*) FROM qa_safe_t2)"), "3");
    assert.match(r, /qa_safe_t1/);
    assert.match(r, /qa_safe_t2/, `the confirmation does not mention qa_safe_t2: ${r}`);
  } finally {
    dbExec("postgres", "DROP TABLE IF EXISTS qa_safe_t1, qa_safe_t2");
  }
}
