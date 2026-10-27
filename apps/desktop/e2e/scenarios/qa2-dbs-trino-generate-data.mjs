// DBS-04 (high): "Generate data…" adds nothing to a Trino table with a date, timestamp, time
// or uuid column. The generated values reach Trino as varchar (and a row(...) column, whose
// type mentions `integer`, gets "Whole number"), and Trino does not cast varchar to those
// types on INSERT: "Insert query has mismatched column types: Table: [bigint, timestamp(3),
// date, uuid, varchar], Query: [integer, varchar(23), varchar(10), varchar(36), …]".
//
// Steps: Trino (docker profile trino), memory.qa_dbs.g (id bigint, ts timestamp(3), d date,
// u uuid, name varchar); navigator › g › Generate data…, keep the suggestions, 10 rows.
// Expected: "Added 10 rows." (typed literals: TIMESTAMP '…', DATE '…', UUID '…', or CASTs).
// Actual: "Nothing was added: … Insert query has mismatched column types …".
// Suspected: the insert of generated rows for the Trino (ANSI) dialect in
// crates/platypus-transfer/src/fake.rs / crates/platypus-sql/src/dml.rs, which writes date
// and time values as plain strings.
import assert from "node:assert/strict";
import { conns, needsPorts, openPath, seedTrinoDriver, sh } from "../qa-dbs.mjs";
import { generate } from "../qa-xfer.mjs";

export const skip = needsPorts(58085);

export function setup({ dir }) {
  sh("trino", `DROP SCHEMA IF EXISTS memory.qa_dbs CASCADE; CREATE SCHEMA memory.qa_dbs;
CREATE TABLE memory.qa_dbs.g (id bigint, ts timestamp(3), d date, u uuid, name varchar);`);
  seedTrinoDriver(dir);
  conns(dir, [["QA Trino", "trino"]]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA Trino", "qa_dbs", "Tables", "g"]);
    const { kinds, out } = await generate(page, "g", ["QA Trino", "qa_dbs"], 10);
    assert.deepEqual(kinds, ["integer", "timestamp", "date", "uuid", "full_name"]);
    assert.match(out, /Added 10 rows/);
    assert.equal(sh("trino", "SELECT count(*) FROM memory.qa_dbs.g;"), "10");
  } finally {
    sh("trino", "DROP SCHEMA IF EXISTS memory.qa_dbs CASCADE;");
  }
}
