// Unit tests of the dashboard's derivations: rates between two snapshots, the cache hit ratio,
// the series and the labels (src/admin/dashboardModel.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  connectionShare,
  gaugeSeries,
  hitRatio,
  latest,
  pushSample,
  ratePerSecond,
  rateSeries,
  rateText,
  ratioSeries,
  uptimeText,
} from "../src/admin/dashboardModel.ts";

const metric = (value, unavailable = null) => ({ value, unavailable });
const sample = (at, extra = {}) => ({
  at,
  metrics: {
    connections_active: metric(2),
    connections_idle: metric(3),
    connections_max: metric(100),
    uptime_seconds: metric(60),
    transactions: metric(1000),
    statements: metric(null, "not counted"),
    cache_hits: metric(900),
    cache_reads: metric(100),
    databases: [],
    databases_unavailable: null,
    replication: [],
    replication_unavailable: null,
    ...extra,
  },
});
const transactions = (m) => m.transactions;

test("a rate is the change of a counter over the seconds between two samples", () => {
  const a = sample(0);
  const b = sample(4000, { transactions: metric(1100) });
  assert.equal(ratePerSecond(a, b, transactions), 25);
});

test("no rate without both values, without time, or when the counter went back", () => {
  const a = sample(0);
  assert.equal(ratePerSecond(a, sample(4000), (m) => m.statements), null);
  assert.equal(ratePerSecond(a, sample(0, { transactions: metric(2000) }), transactions), null);
  assert.equal(ratePerSecond(a, sample(1000, { transactions: metric(5) }), transactions), null);
});

test("the hit ratio is that of the reads between two samples", () => {
  const a = sample(0);
  const b = sample(1000, { cache_hits: metric(1000), cache_reads: metric(200) });
  // 100 hits and 100 reads in between.
  assert.equal(hitRatio(a, b), 0.5);
});

test("with no read in between, the hit ratio is that since the start", () => {
  assert.equal(hitRatio(sample(0), sample(1000)), 0.9);
  assert.equal(hitRatio(null, sample(1000)), 0.9);
  assert.equal(hitRatio(null, sample(0, { cache_hits: metric(0), cache_reads: metric(0) })), null);
  assert.equal(hitRatio(null, sample(0, { cache_hits: metric(null, "no") })), null);
});

test("a hit ratio after a reset of the counters falls back to the total", () => {
  const b = sample(1000, { cache_hits: metric(10), cache_reads: metric(10) });
  assert.equal(hitRatio(sample(0), b), 0.5);
});

test("series start at the second sample for rates", () => {
  const samples = [sample(0), sample(1000, { transactions: metric(1010) }), sample(3000, { transactions: metric(1050) })];
  assert.deepEqual(rateSeries(samples, transactions), [10, 20]);
  assert.equal(latest(rateSeries(samples, transactions)), 20);
  assert.equal(ratioSeries(samples).length, 2);
  assert.deepEqual(gaugeSeries(samples, (m) => m.connections_active), [2, 2, 2]);
  assert.equal(latest([]), null);
});

test("samples are capped, and a clock set back starts again", () => {
  let samples = [];
  for (let i = 0; i < 5; i++) samples = pushSample(samples, sample(i * 1000), 3);
  assert.deepEqual(samples.map((s) => s.at), [2000, 3000, 4000]);
  samples = pushSample(samples, sample(100), 3);
  assert.deepEqual(samples.map((s) => s.at), [100]);
});

test("the share of connections used", () => {
  assert.equal(connectionShare(sample(0).metrics), 0.05);
  assert.equal(connectionShare(sample(0, { connections_max: metric(0) }).metrics), null);
  assert.equal(connectionShare(sample(0, { connections_idle: metric(null, "x") }).metrics), null);
  assert.equal(connectionShare(sample(0, { connections_max: metric(3) }).metrics), 1);
});

test("uptime reads with its two biggest units", () => {
  assert.equal(uptimeText(45), "45s");
  assert.equal(uptimeText(125), "2m 5s");
  assert.equal(uptimeText(7500), "2h 5m");
  assert.equal(uptimeText(3 * 86400 + 4 * 3600 + 59), "3d 4h");
  assert.equal(uptimeText(-5), "0s");
});

test("rates read with a decimal only when small", () => {
  assert.equal(rateText(0.256), "0.3");
  assert.equal(rateText(9.96), "10.0");
  assert.equal(rateText(1234.5), "1235");
});
