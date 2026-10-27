// DBS-08 (medium): "Generate data…" adds nothing to a TimescaleDB hypertable keyed by
// (device, ts), the usual shape of one: every primary key column that is not an integer or a
// uuid gets "Next key (after the largest)", which writes numbers, so the timestamptz key fails
// ("column "ts" is of type timestamp with time zone but expression is of type integer"). The
// same happens on PostgreSQL to any table whose key holds a date or a timestamp.
//
// Steps: TimescaleDB (docker profile pgwire, port 55433), database qa_dbs_ts with the
// extension; metrics (ts timestamptz NOT NULL, device text NOT NULL, temp double precision,
// PRIMARY KEY (device, ts)) made a hypertable; navigator › metrics › Generate data…, keep the
// suggestions, 20 rows.
// Expected: a timestamp generator for ts (unique values), "Added 20 rows.".
// Actual: ts gets "Next key", "Nothing was added: column "ts" is of type timestamp with time
// zone but expression is of type integer".
// Suspected: apps/desktop/src/transfer/generators.ts:56 (`next_key` for any key column that is
// neither an integer nor a uuid).
import assert from "node:assert/strict";
import { conns, dropTimescaleDb, needsPorts, openPath, sh, timescaleDb } from "../qa-dbs.mjs";
import { generate } from "../qa-xfer.mjs";

export const skip = needsPorts(55433);

export function setup({ dir }) {
  timescaleDb(`CREATE TABLE metrics (ts timestamptz NOT NULL, device text NOT NULL, temp double precision, PRIMARY KEY (device, ts));
SELECT create_hypertable('metrics', 'ts', chunk_time_interval => interval '1 day');`);
  conns(dir, [["QA TS", "timescale"]]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA TS", "qa_dbs_ts", "public", "Tables", "metrics"]);
    const { kinds, out } = await generate(page, "metrics", ["QA TS", "qa_dbs_ts", "public"], 20);
    assert.match(out, /Added 20 rows/, `${out} (generators ${kinds})`);
    assert.equal(sh("timescale", "SELECT count(*) FROM metrics"), "20");
  } finally {
    dropTimescaleDb();
  }
}
