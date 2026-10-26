/** Scales, ticks, labels and shapes of the chart (`Chart.tsx`). Pure: the unit tests import it. */

const DAY = 86_400_000;

/** Maps `domain` onto `range` linearly; a domain of one value maps to the middle. */
export function linear([d0, d1]: [number, number], [r0, r1]: [number, number]): (v: number) => number {
  if (d1 === d0) return () => (r0 + r1) / 2;
  const k = (r1 - r0) / (d1 - d0);
  return (v) => r0 + (v - d0) * k;
}

/** The smallest and the largest value; `[0, 1]` for none. */
export function extent(values: readonly number[]): [number, number] {
  let lo = Infinity;
  let hi = -Infinity;
  for (const v of values) {
    if (v < lo) lo = v;
    if (v > hi) hi = v;
  }
  return lo <= hi ? [lo, hi] : [0, 1];
}

/** A step of 1, 2 or 5 times a power of ten that cuts `min..max` in about `count` parts. */
function step(min: number, max: number, count: number): number {
  const raw = (max - min) / Math.max(1, count);
  const power = 10 ** Math.floor(Math.log10(raw));
  return [1, 2, 5, 10].map((m) => m * power).find((s) => s >= raw) ?? 10 * power;
}

/** Round values within `min..max`, about `count` of them. */
export function niceTicks(min: number, max: number, count: number): number[] {
  if (!Number.isFinite(min) || !Number.isFinite(max)) return [];
  if (max <= min) return [min];
  const s = step(min, max, count);
  const ticks: number[] = [];
  for (let v = Math.ceil(min / s) * s; v <= max + s * 1e-9; v += s) ticks.push(Number(v.toPrecision(12)));
  return ticks;
}

/** `min..max` widened to round ticks, so that the axis starts and ends on one; a single value
 *  goes from 0. */
export function niceDomain(min: number, max: number, count: number): [number, number] {
  if (!Number.isFinite(min) || !Number.isFinite(max)) return [0, 1];
  if (max <= min) return min === 0 ? [0, 1] : [Math.min(0, min), Math.max(0, max)];
  const s = step(min, max, count);
  return [Math.floor(min / s) * s, Math.ceil(max / s) * s];
}

/** Ticks of a time axis in milliseconds: whole days when it spans days, else round numbers. */
export function timeTicks(min: number, max: number, count: number): number[] {
  if (max - min < 2 * DAY) return niceTicks(min, max, count);
  return niceTicks(min / DAY, max / DAY, count)
    .filter((d) => Number.isInteger(d))
    .map((d) => d * DAY);
}

/** Milliseconds since the epoch as a date, with the time when the axis spans less than two
 *  days. In UTC: the backend reads times without their zone. */
export function formatTime(ms: number, span: number): string {
  const iso = new Date(ms).toISOString();
  return span < 2 * DAY ? iso.slice(0, 16).replace("T", " ") : iso.slice(0, 10);
}

const compact = new Map<string, Intl.NumberFormat>();

/** A number in a few characters: 1.5K, 2M, 0.25. */
export function formatNumber(v: number, locale?: string): string {
  const key = locale ?? "";
  let format = compact.get(key);
  if (!format) {
    format = new Intl.NumberFormat(locale, { notation: "compact", maximumFractionDigits: 2 });
    compact.set(key, format);
  }
  return format.format(v);
}

/** `n` equal bands from `start` to `end`, each with `padding` (a share of it) left empty. */
export function band(n: number, start: number, end: number, padding: number) {
  const size = (end - start) / Math.max(1, n);
  return { step: size, width: size * (1 - padding), at: (i: number) => start + size * i + (size * padding) / 2 };
}

/** Which of `n` categories get a label when only `room` fit: every k-th from the first. */
export function categoryTicks(n: number, room: number): number[] {
  const every = Math.max(1, Math.ceil(n / Math.max(1, room)));
  const ticks: number[] = [];
  for (let i = 0; i < n; i += every) ticks.push(i);
  return ticks;
}

/** Start and end angle of each slice, clockwise from the top; negative values count as 0. */
export function pieAngles(values: readonly number[]): [number, number][] {
  const total = values.reduce((sum, v) => sum + Math.max(0, v), 0);
  let at = -Math.PI / 2;
  return values.map((v) => {
    const start = at;
    at += total > 0 ? (Math.max(0, v) / total) * 2 * Math.PI : 0;
    return [start, at];
  });
}

/** The SVG path of the slice from angle `a0` to `a1`; a full turn is a whole disc. */
export function arcPath(cx: number, cy: number, r: number, a0: number, a1: number): string {
  const point = (a: number) => `${(cx + r * Math.cos(a)).toFixed(2)} ${(cy + r * Math.sin(a)).toFixed(2)}`;
  if (a1 - a0 >= 2 * Math.PI - 1e-9) {
    return `M ${point(a0)} A ${r} ${r} 0 1 1 ${point(a0 + Math.PI)} A ${r} ${r} 0 1 1 ${point(a0)} Z`;
  }
  const large = a1 - a0 > Math.PI ? 1 : 0;
  return `M ${cx} ${cy} L ${point(a0)} A ${r} ${r} 0 ${large} 1 ${point(a1)} Z`;
}

/** `text` cut to `max` characters with an ellipsis. */
export function truncate(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}
