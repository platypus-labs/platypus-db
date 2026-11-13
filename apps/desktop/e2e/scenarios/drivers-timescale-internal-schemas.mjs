// DBS-15 (low): on TimescaleDB the extension's own schemas (_timescaledb_cache,
// _timescaledb_catalog, _timescaledb_config, _timescaledb_functions, _timescaledb_internal,
// timescaledb_experimental, timescaledb_information) are listed as user schemas, before and
// after `public`, not muted and not hidden by the Simple view, while information_schema and
// pg_catalog are. The extension's ~90 functions and ~13 procedures also fill public's
// Functions and Procedures folders (they belong to the extension: pg_depend deptype 'e').
// Same root cause as the CockroachDB system schemas (navigator-cockroach-system-schemas).
//
// Steps: TimescaleDB (docker profile pgwire, port 55433), a database with the extension;
// expand the connection and the database.
// Expected: the extension's schemas muted like the catalog schemas (DBeaver groups them as
// system schemas too).
// Actual: shown as ordinary schemas.
// Suspected: crates/platypus-driver-pg/src/meta.rs `schemas` (only pg_catalog and
// information_schema are flagged as system; schemas owned by an extension are not).
import assert from "node:assert/strict";
import { conns, dropTimescaleDb, needsPorts, openPath, timescaleDb, treeRow } from "../drivers-helpers.mjs";

export const skip = needsPorts(55433);

export function setup({ dir }) {
  timescaleDb("CREATE TABLE plain (id int PRIMARY KEY);");
  conns(dir, [["QA TS", "timescale"]]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA TS", "qa_dbs_ts", "_timescaledb_internal"]);
    await page.waitFor(`${treeRow("pg_catalog")} !== undefined`, 15_000);
    assert.ok(await page.eval(`${treeRow("pg_catalog")}.classList.contains('muted')`), "pg_catalog is muted");
    for (const schema of ["_timescaledb_catalog", "_timescaledb_internal", "timescaledb_information"]) {
      assert.ok(await page.eval(`${treeRow(schema)}.classList.contains('muted')`), `${schema} is a system schema`);
    }
  } finally {
    dropTimescaleDb();
  }
}
