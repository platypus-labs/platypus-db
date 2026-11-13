// What the admin tab's dashboard derives from the server's snapshots: rates between two
// counters, the cache hit ratio, and the series the sparklines draw. Types only from the app:
// the unit tests load this module without it.
import type { Metric, ServerMetrics } from "../api";

/** A snapshot with the time it was read, in milliseconds. */
export interface Sample {
  at: number;
  metrics: ServerMetrics;
}

/** How many samples the dashboard keeps: the sparklines show the last of them. */
const MAX_SAMPLES = 60;

/** `samples` with `next` last, the oldest dropped beyond `max`. */
export function pushSample(samples: Sample[], next: Sample, max = MAX_SAMPLES): Sample[] {
  // A clock set back would make the intervals negative: start again.
  const last = samples[samples.length - 1];
  const kept = last && next.at <= last.at ? [] : samples;
  return [...kept, next].slice(-max);
}

/** Which counter of a snapshot. */
type Pick = (m: ServerMetrics) => Metric;

/** The change per second of the counter `pick` between two samples; `null` when either has
 *  none, no time passed, or the counter went back (the server restarted or reset it). */
export function ratePerSecond(prev: Sample, cur: Sample, pick: Pick): number | null {
  const [a, b] = [pick(prev.metrics).value, pick(cur.metrics).value];
  const seconds = (cur.at - prev.at) / 1000;
  if (a === null || b === null || seconds <= 0 || b < a) return null;
  return (b - a) / seconds;
}

/** The share of page reads found in the cache between two samples, 0 to 1. With no read in
 *  between, the share since the server started; `null` when the server counts none. */
export function hitRatio(prev: Sample | null, cur: Sample): number | null {
  const hits = cur.metrics.cache_hits.value;
  const reads = cur.metrics.cache_reads.value;
  if (hits === null || reads === null) return null;
  if (prev) {
    const [h0, r0] = [prev.metrics.cache_hits.value, prev.metrics.cache_reads.value];
    if (h0 !== null && r0 !== null && hits >= h0 && reads >= r0 && hits - h0 + (reads - r0) > 0) {
      return (hits - h0) / (hits - h0 + (reads - r0));
    }
  }
  return hits + reads > 0 ? hits / (hits + reads) : null;
}

/** Each sample after the first with the one before it. */
function pairs(samples: Sample[]): [Sample, Sample][] {
  return samples.flatMap((cur, i) => {
    const prev = samples[i - 1];
    return prev ? [[prev, cur] as [Sample, Sample]] : [];
  });
}

/** The rate of `pick` at each sample after the first. */
export function rateSeries(samples: Sample[], pick: Pick): (number | null)[] {
  return pairs(samples).map(([prev, cur]) => ratePerSecond(prev, cur, pick));
}

/** The hit ratio at each sample after the first. */
export function ratioSeries(samples: Sample[]): (number | null)[] {
  return pairs(samples).map(([prev, cur]) => hitRatio(prev, cur));
}

/** The value of `pick` at each sample. */
export const gaugeSeries = (samples: Sample[], pick: Pick): (number | null)[] => samples.map((s) => pick(s.metrics).value);

/** The last number of `series`, if it has one. */
export const latest = (series: (number | null)[]): number | null => series[series.length - 1] ?? null;

/** The share of the connections the server accepts that are open, 0 to 1; `null` when either
 *  is unknown or the limit is not positive. */
export function connectionShare(m: ServerMetrics): number | null {
  const [active, idle, max] = [m.connections_active.value, m.connections_idle.value, m.connections_max.value];
  if (active === null || idle === null || max === null || max <= 0) return null;
  return Math.min(1, (active + idle) / max);
}

/** `seconds` as the two biggest units: `3d 4h`, `2h 5m`, `45s`. */
export function uptimeText(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const [d, h, m] = [Math.floor(s / 86400), Math.floor((s % 86400) / 3600), Math.floor((s % 3600) / 60)];
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s % 60}s`;
  return `${s}s`;
}

/** A rate for a tile: one decimal under 10, whole numbers above. */
export function rateText(rate: number): string {
  return rate < 10 ? rate.toFixed(1) : Math.round(rate).toString();
}

/** How much time the samples cover: whole seconds up to a minute, else rounded minutes. */
export function spanText(samples: Sample[]): { unit: "s" | "min"; n: number } {
  const first = samples[0];
  const last = samples[samples.length - 1];
  const seconds = first && last ? Math.max(0, Math.round((last.at - first.at) / 1000)) : 0;
  return seconds < 60 ? { unit: "s", n: seconds } : { unit: "min", n: Math.round(seconds / 60) };
}
