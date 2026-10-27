// DBS-17 (low): on YugabyteDB, Generate DDL drops the sharding of keys and indexes: a range
// sharded key `PRIMARY KEY (id ASC)` comes out as `PRIMARY KEY (id)` (which YugabyteDB shards
// by HASH), and `SPLIT AT VALUES (…)` / `SPLIT INTO n TABLETS` are lost, so the DDL recreates a
// differently distributed table (range scans on id become full scans). Index columns keep
// their HASH/ASC (pg_get_indexdef has them); the key does not.
//
// Steps: YugabyteDB (docker profile pgwire, port 55434); CREATE TABLE qa_dbs.r (id bigint,
// v text, PRIMARY KEY (id ASC)) SPLIT AT VALUES ((100)); navigator › r › Generate DDL.
// Expected: PRIMARY KEY (id ASC) and the SPLIT AT clause (as ysql_dump writes).
// Actual: CONSTRAINT r_pkey PRIMARY KEY (id).
// Suspected: crates/platypus-driver-pg/src/ddl.rs (constraints from pg_constraint column
// lists; YugabyteDB's yb_get_range_split_clause / pg_get_constraintdef are not used).
import assert from "node:assert/strict";
import { conns, editorText, needsPorts, openPath, sh, treeMenu } from "../qa-dbs.mjs";

export const skip = needsPorts(55434);

export function setup({ dir }) {
  sh("yugabyte", "DROP SCHEMA IF EXISTS qa_dbs CASCADE; CREATE SCHEMA qa_dbs; CREATE TABLE qa_dbs.r (id bigint, v text, PRIMARY KEY (id ASC)) SPLIT AT VALUES ((100));");
  conns(dir, [["QA YB", "yugabyte"]]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA YB", "yugabyte", "qa_dbs", "Tables", "r"]);
    await treeMenu(page, "r", "Generate DDL");
    await page.waitFor(`(() => { try { return ${editorText}.includes('CREATE TABLE'); } catch { return false; } })()`, 15_000);
    const ddl = await page.eval(editorText);
    assert.match(ddl, /PRIMARY KEY \(id ASC\)/, ddl);
  } finally {
    sh("yugabyte", "DROP SCHEMA IF EXISTS qa_dbs CASCADE;");
  }
}
