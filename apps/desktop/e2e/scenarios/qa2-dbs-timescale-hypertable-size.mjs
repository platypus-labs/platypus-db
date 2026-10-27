// DBS-16 (low): the details of a TimescaleDB hypertable show "Table size 0 bytes", a total of a
// few kB and "Estimated rows unknown", however much data it holds: the sizes are those of the
// empty parent table, while the rows live in its chunks (_timescaledb_internal._hyper_*), and
// the chunks are not listed anywhere. Generate DDL also writes a plain CREATE TABLE, without
// create_hypertable() or the compression settings, so it does not recreate the hypertable.
//
// Steps: TimescaleDB (docker profile pgwire, port 55433), database with the extension;
// metrics hypertable with 97 rows (5 daily chunks, 2 compressed); navigator › metrics › Open
// details.
// Expected: the hypertable's size (hypertable_size('metrics'), here several hundred kB) and
// its row estimate; ideally its chunks and compression state, as DBeaver's TimescaleDB view.
// Actual: Table size 0 bytes, Total size 24 kB, Estimated rows 0 (after ANALYZE).
// Suspected: crates/platypus-driver-pg/src/meta.rs:371 (pg_total_relation_size of the parent
// only) and ddl.rs (no TimescaleDB awareness).
import assert from "node:assert/strict";
import { conns, dropTimescaleDb, needsPorts, openPath, timescaleDb } from "../qa-dbs.mjs";
import { menu } from "../qa-xfer.mjs";

export const skip = needsPorts(55433);

export function setup({ dir }) {
  timescaleDb(`CREATE TABLE metrics (ts timestamptz NOT NULL, device text NOT NULL, temp double precision, PRIMARY KEY (device, ts));
SELECT create_hypertable('metrics', 'ts', chunk_time_interval => interval '1 day');
INSERT INTO metrics SELECT t, 'dev' || (i % 3), i FROM generate_series(timestamptz '2025-01-01', timestamptz '2025-01-05', interval '1 hour') WITH ORDINALITY AS g(t, i);
ANALYZE metrics;`);
  conns(dir, [["QA TS", "timescale"]]);
}

const objectTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;

export default async function (page) {
  try {
    await openPath(page, ["QA TS", "qa_dbs_ts", "public", "Tables", "metrics"]);
    await menu(page, "metrics", "Open details", null, ["QA TS", "qa_dbs_ts", "public", "Tables"]);
    await page.waitFor(`${objectTab}?.textContent.includes('Table size')`, 15_000);
    const text = await page.eval(`${objectTab}.textContent`);
    const size = text.match(/Table size([\d.,]+ (?:bytes|kB|MB|GB))/)?.[1];
    assert.ok(size && size !== "0 bytes", `the hypertable's data size: ${size}`);
  } finally {
    dropTimescaleDb();
  }
}
