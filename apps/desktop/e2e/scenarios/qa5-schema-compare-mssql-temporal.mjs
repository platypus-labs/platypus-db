// QA5-S15 (medium). Compare with... on SQL Server creates a system-versioned (temporal) table as two
// ordinary tables: the history is no longer kept, and the history table becomes a table of its own.
// Steps: tmp with PERIOD FOR SYSTEM_TIME and SYSTEM_VERSIONING = ON (history tmp_hist); compare with an empty schema; run.
// Expected: tmp is a temporal table in the target (or the script says it cannot be). Actual: NON_TEMPORAL_TABLE.
// Code: crates/platypus-schema (period columns and WITH (SYSTEM_VERSIONING ...) are not read; TableDesign.options is empty for SQL Server).
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../qa3-schema.mjs";

export const skip = needsEngines("mssql");
const e = ENGINES.mssql;

/** A versioned table cannot be dropped: switch versioning off first, if the table is there. */
const unversion = (schema) => {
  try {
    e.exec("", schema, `IF OBJECT_ID('${schema}.tmp') IS NOT NULL AND OBJECTPROPERTY(OBJECT_ID('${schema}.tmp'), 'TableTemporalType') = 2 ALTER TABLE ${schema}.tmp SET (SYSTEM_VERSIONING = OFF);`);
  } catch {
    // The schema is not there yet.
  }
};

export function setup({ dir }) {
  unversion(SRC);
  e.seed(dir, SRC, `CREATE TABLE S.tmp (id int PRIMARY KEY, v int, vf datetime2 GENERATED ALWAYS AS ROW START NOT NULL DEFAULT SYSUTCDATETIME(), vt datetime2 GENERATED ALWAYS AS ROW END NOT NULL DEFAULT CONVERT(datetime2, '9999-12-31 23:59:59.9999999'), PERIOD FOR SYSTEM_TIME (vf, vt)) WITH (SYSTEM_VERSIONING = ON (HISTORY_TABLE = S.tmp_hist));`);
  e.seed(dir, DST, "");
  saveEngines(dir, ["mssql"]);
}

export default async function (page) {
  const p = PLACES.mssql;
  try {
    await openPath(page, [...p.chain, p.last]);
    const r = await roundTrip(page, p.chain, p.last, p.target, p.targetSchema);
    assert.ok(r.sql, `Compare gave no script: ${JSON.stringify(r.first)}`);
    assert.equal(r.runError, null, r.sql);
    const kind = e.query(`SELECT temporal_type_desc FROM sys.tables WHERE name = 'tmp' AND schema_id = SCHEMA_ID('${DST}')`).trim();
    assert.equal(kind, "SYSTEM_VERSIONED_TEMPORAL_TABLE", r.sql);
  } finally {
    unversion(SRC);
    unversion(DST);
    e.drop(SRC);
    e.drop(DST);
  }
}
