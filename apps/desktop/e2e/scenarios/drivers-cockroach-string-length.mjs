// DBS-07 (medium): on CockroachDB a STRING(n) column (CockroachDB's usual spelling of
// VARCHAR(n)) is shown as "text(n+4)" in the navigator and the table editor, and Generate DDL
// writes `name text(44)`, which neither CockroachDB nor PostgreSQL accepts ("at or near "(":
// syntax error"), so the DDL cannot recreate the table.
//
// Steps: CockroachDB (docker profile pgwire, port 56257); CREATE TABLE qa_dbs.s (id INT8
// PRIMARY KEY, name STRING(40)); navigator › s › Columns, then Generate DDL.
// Expected: name shown as character varying(40) (or STRING(40)); the DDL runs.
// Actual: "text(44)" everywhere.
// Cause: CockroachDB reports STRING(n) as atttypid 25 (text) with atttypmod n+4, and its
// format_type(25, 44) answers "text(44)". The PostgreSQL driver uses format_type as is:
// crates/platypus-driver-pg/src/meta.rs:259 and ddl.rs:147 (CockroachDB: map text with a
// typmod to character varying(typmod - 4)).
import assert from "node:assert/strict";
import { conns, editorText, needsPorts, openPath, sh, treeMenu } from "../drivers-helpers.mjs";

export const skip = needsPorts(56257);

export function setup({ dir }) {
  sh("cockroach", "DROP SCHEMA IF EXISTS qa_dbs CASCADE; CREATE SCHEMA qa_dbs; CREATE TABLE qa_dbs.s (id INT8 PRIMARY KEY, name STRING(40));");
  conns(dir, [["QA CRDB", "cockroach"]]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA CRDB", "platypus", "qa_dbs", "Tables", "s", "Columns", "name"]);
    const column = await page.eval(`[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === 'name').textContent`);
    assert.doesNotMatch(column, /text\(44\)/, `the column's type: ${column}`);
    await treeMenu(page, "s", "Generate DDL");
    await page.waitFor(`(() => { try { return ${editorText}.includes('CREATE TABLE'); } catch { return false; } })()`, 15_000);
    const ddl = await page.eval(editorText);
    assert.doesNotMatch(ddl, /text\(44\)/, ddl);
    sh("cockroach", `${ddl.replaceAll("qa_dbs.s ", "qa_dbs.s2 ").replace(/CREATE INDEX[\s\S]*$/, "")};`.replace(/;\s*;$/, ";"));
  } finally {
    sh("cockroach", "DROP SCHEMA IF EXISTS qa_dbs CASCADE;");
  }
}
