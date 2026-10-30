/** The lines of the foreign keys of an ER diagram, routed around the tables: a line leaves a
 *  box on the side facing the other, runs level and turns only in the free channels between
 *  the boxes, crossing each band of boxes in its way through a gap between them. Works on any
 *  positions, the automatic layout's (which leaves a gap in every column a line crosses) or
 *  the user's. */

import type { Point } from "./layout";

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** A line from the row at `fromRow` (from the top of the box) of `from` to the row at `toRow`
 *  of `to`. */
export interface Wire {
  from: string;
  fromRow: number;
  to: string;
  toRow: number;
}

/** How far a line keeps from a box it passes by. */
const MARGIN = 8;
/** The shortest straight piece leaving or entering a box, for the end markers. */
const STUB = 12;
/** The distance between two lines sharing a channel or a gap. */
const TRACK = 6;
/** How far right of a box a line to the same box (or a box above or below) goes. */
const LOOP = 20;
/** How far down a line from a row to the same row goes before coming back. */
const ROW_LOOP = 10;
/** How much longer a line may get to run with the others of its bundle. */
const BUS_DETOUR = 80;
/** The highest a line goes, just inside the drawing. */
const TOP = 2;

interface Span {
  lo: number;
  hi: number;
}

/** A band of boxes in the way: its extent across, and the free stretches along it. */
interface Band extends Span {
  blocked: Span[];
}

/** A segment already drawn at `at` (across a gap or a channel), covering `lo`..`hi` along it. */
interface Lane extends Span {
  at: number;
  bundle?: string;
}

/** The segments already drawn in each gap, to put the next line beside them (or on the one of
 *  its bundle). */
type Used = Map<string, Lane[]>;

/** The lines ending on the same row of the same box travel together as far as they can, like a
 *  bus: a line's bundle names that end, and says whether it is where the line starts (the
 *  lines fan out of it) or where it ends (they gather into it). */
interface Bundle {
  key: string;
  at: "start" | "end";
}

/** A turn of a line in a channel between bands of boxes: from the level `from` (coming from the
 *  left) to `to` (going on right), somewhere between `lo` and `hi` across. Where exactly is
 *  decided once all the lines are known, to order the turns sharing the channel. */
interface Turn extends Span {
  from: number;
  to: number;
  points: [Point, Point];
  bundle: Bundle;
}

/** The points of every wire, from its `from` box to its `to` box; a wire whose box is missing
 *  gets no points. */
export function routeWires(rects: Record<string, Rect>, wires: Wire[]): Point[][] {
  const all = Object.values(rects);
  const turns = new Map<string, Turn[]>();
  const gaps: Used = new Map();
  const loops = new Map<number, Lane[]>();
  const lines = wires.map((w) => {
    const a = rects[w.from];
    const b = rects[w.to];
    if (!a || !b) return [];
    const ya = a.y + w.fromRow;
    const yb = b.y + w.toRow;
    const key = `${w.to}:${Math.round(yb)}`;
    if (a.x + a.width + 2 * STUB <= b.x) {
      return route(all, a, b, { x: a.x + a.width, y: ya }, { x: b.x, y: yb }, { key: `${key}:l`, at: "end" }, turns, gaps);
    }
    if (b.x + b.width + 2 * STUB <= a.x) {
      return route(all, a, b, { x: b.x + b.width, y: yb }, { x: a.x, y: ya }, { key: `${key}:r`, at: "start" }, turns, gaps).reverse();
    }
    // The boxes overlap across (the same box, or one above the other): out on their right
    // sides, and down a little when a row goes back to itself.
    const side = Math.max(a.x + a.width, b.x + b.width);
    const back = ya === yb ? ya + ROW_LOOP : yb;
    const along = { lo: Math.min(ya, back), hi: Math.max(ya, back) };
    const x = nearestFree(loops.get(Math.round(side)) ?? [], side + LOOP, { lo: side + STUB, hi: side + LOOP * 2 }, along);
    add(loops, Math.round(side), { at: x, ...along });
    return [{ x: a.x + a.width, y: ya }, { x, y: ya }, { x, y: back }, { x: b.x + b.width, y: back }];
  });
  for (const list of turns.values()) placeTurns(list);
  return lines;
}

/** A cheap line for `wire`, not avoiding the other boxes: out of the facing sides, with one turn
 *  halfway between them (on the right sides when the boxes overlap across). For the wires of a
 *  box being dragged, until it is dropped and every line is routed. */
export function quickWire(rects: Record<string, Rect>, w: Wire): Point[] {
  const a = rects[w.from];
  const b = rects[w.to];
  if (!a || !b) return [];
  const ya = a.y + w.fromRow;
  const yb = b.y + w.toRow;
  if (a.x + a.width <= b.x) {
    const x = (a.x + a.width + b.x) / 2;
    return [{ x: a.x + a.width, y: ya }, { x, y: ya }, { x, y: yb }, { x: b.x, y: yb }];
  }
  if (b.x + b.width <= a.x) return quickWire(rects, { from: w.to, fromRow: w.toRow, to: w.from, toRow: w.fromRow }).reverse();
  const x = Math.max(a.x + a.width, b.x + b.width) + LOOP;
  return [{ x: a.x + a.width, y: ya }, { x, y: ya }, { x, y: yb }, { x: b.x + b.width, y: yb }];
}

/** Puts the turns sharing a channel side by side, in the order that crosses least: of the lines
 *  going down, the one coming from lower or going lower turns first (so a fan out of one row
 *  does not cross itself), and the other way round for the lines going up. The turns of a
 *  bundle fanning out of one level (or gathering into one) share their place. */
function placeTurns(list: Turn[]): void {
  const lo = Math.max(...list.map((t) => t.lo));
  const hi = Math.max(lo, Math.min(...list.map((t) => t.hi)));
  const groups = new Map<string, Turn[]>();
  for (const t of list) add(groups, `${t.bundle.key}/${t.bundle.at === "start" ? `f${t.from}` : `t${t.to}`}`, t);
  // A group goes where its mean turn would.
  const mean = [...groups.values()].map((g) => ({
    turns: g,
    from: g.reduce((s, t) => s + t.from, 0) / g.length,
    to: g.reduce((s, t) => s + t.to, 0) / g.length,
  }));
  const down = (t: { from: number; to: number }) => t.to > t.from;
  mean.sort((s, t) => Number(down(t)) - Number(down(s)) || (down(s) ? t.from + t.to - s.from - s.to : s.from + s.to - t.from - t.to));
  let step = Math.min(2 * TRACK, (hi - lo) / (mean.length + 1));
  let first = (lo + hi) / 2 - (step * (mean.length - 1)) / 2;
  // A channel with no width (boxes the user has put over the line's start) would stack the
  // turns at one x: they keep a small distance, spread back from the channel's edge by at most
  // the margin kept from boxes.
  if (hi === lo) {
    step = Math.min(TRACK / 2, MARGIN / Math.max(1, mean.length - 1));
    first = lo - step * (mean.length - 1);
  }
  for (const [i, g] of mean.entries()) for (const t of g.turns) for (const p of t.points) p.x = first + i * step;
}

/** The route from `start` (on the right side of the left box) to `end` (on the left side of the
 *  right box), through the bands of boxes between them: level while it can, turning in the
 *  channel before a band that blocks its level to a gap of that band. Only the boxes within a
 *  window above and below the two ends count, so that tables far away (a grid of tables below
 *  the others) do not close the channels between columns; the line stays in the window, which
 *  is widened when a band blocks all of it. */
function route(all: Rect[], a: Rect, b: Rect, start: Point, end: Point, bundle: Bundle, turns: Map<string, Turn[]>, gaps: Used): Point[] {
  for (const slack of WINDOWS) {
    const found = attempt(all, a, b, start, end, bundle, gaps, { lo: Math.min(start.y, end.y) - slack, hi: Math.max(start.y, end.y) + slack });
    if (!found) continue;
    for (const [key, t] of found.turns) add(turns, key, t);
    for (const [key, lane] of found.lanes) add(gaps, key, lane);
    return found.points;
  }
  return [start, end];
}

/** How far above and below its ends a line looks for its way, the next tried when one fails. */
const WINDOWS = [300, 1500, Infinity];

/** A route within `window`, with the turns and the gap levels it takes (to add once it is
 *  kept), or `null` when a band blocks the whole window. */
function attempt(all: Rect[], a: Rect, b: Rect, start: Point, end: Point, bundle: Bundle, gaps: Used, window: Span) {
  const points = [start];
  const turns: [string, Turn][] = [];
  const lanes: [string, Lane][] = [];
  let y = start.y;
  let left = start.x + STUB;
  const turn = (right: number, key: number, to: number) => {
    const t: Turn = { lo: left, hi: Math.max(left, right), from: y, to, points: [{ x: left, y }, { x: left, y: to }], bundle };
    // Keyed by where the channel ends, the left side of the next column of boxes.
    turns.push([String(Math.round(key)), t]);
    points.push(...t.points);
    y = to;
  };
  for (const band of bandsBetween(all, a, b, start.x, end.x, window)) {
    if (!isBlocked(band.blocked, y)) {
      left = Math.max(left, band.hi);
      continue;
    }
    const gap = gapNear(gaps, band, y, end.y, bundle.key, window);
    if (!gap) return null;
    if (gap.lane) lanes.push([gap.key, gap.lane]);
    turn(band.lo, band.lo + MARGIN, gap.at);
    left = Math.max(left, band.hi);
  }
  if (y !== end.y) turn(end.x - STUB, end.x, end.y);
  points.push(end);
  return { points, turns, lanes };
}

/** The boxes between `x0` and `x1` within `window` (the two ends' boxes left out), merged in
 *  bands that overlap across, left to right, each with the stretches its boxes block along it. */
function bandsBetween(all: Rect[], a: Rect, b: Rect, x0: number, x1: number, window: Span): Band[] {
  // A box only touching an end (in the column of the box the line enters) is not in the way.
  const inWay = all
    .filter((r) => r !== a && r !== b && r.x < x1 && r.x + r.width > x0 && r.y - MARGIN < window.hi && r.y + r.height + MARGIN > window.lo)
    .sort((p, q) => p.x - q.x);
  const bands: (Band & { boxes: Rect[] })[] = [];
  for (const r of inWay) {
    const lo = Math.max(x0, r.x - MARGIN);
    const hi = Math.min(x1, r.x + r.width + MARGIN);
    const last = bands[bands.length - 1];
    if (last && lo <= last.hi) {
      last.hi = Math.max(last.hi, hi);
      last.boxes.push(r);
    } else bands.push({ lo, hi, blocked: [], boxes: [r] });
  }
  for (const band of bands) {
    const spans = band.boxes.map((r) => ({ lo: r.y - MARGIN, hi: r.y + r.height + MARGIN })).sort((p, q) => p.lo - q.lo);
    for (const s of spans) {
      const last = band.blocked[band.blocked.length - 1];
      if (last && s.lo <= last.hi) last.hi = Math.max(last.hi, s.hi);
      else band.blocked.push({ ...s });
    }
  }
  return bands;
}

/** Adds `value` to the list of `key`. */
function add<K, V>(map: Map<K, V[]>, key: K, value: V): void {
  const list = map.get(key);
  if (list) list.push(value);
  else map.set(key, [value]);
}

const isBlocked = (blocked: Span[], y: number) => blocked.some((s) => y > s.lo && y < s.hi);

/** The level at which a line at `y` heading for `target` crosses a band, within `window`: in
 *  the free stretch that adds the least climbing, nearest to `y`, beside the lines already
 *  crossing there (`lane`, to add to them); or on a line of its bundle crossing there, when that
 *  is not much of a detour. `null` when the band blocks the whole window. */
function gapNear(gaps: Used, band: Band, y: number, target: number, bundle: string, window: Span): { at: number; key: string; lane?: Lane } | null {
  const free: Span[] = [];
  let from = Math.max(TOP, window.lo);
  for (const s of band.blocked) {
    if (s.lo > from) free.push({ lo: from, hi: Math.min(s.lo, window.hi) });
    from = Math.max(from, s.hi);
  }
  free.push({ lo: from, hi: window.hi });
  const usable = free.filter((s) => s.hi > s.lo);
  if (usable.length === 0) return null;
  const cost = (at: number) => Math.abs(at - y) + Math.abs(at - target);
  let best = { cost: Infinity, at: 0, span: usable[0]! };
  for (const s of usable) {
    // The point of the stretch nearest to `y`: on the way to the target when the stretch
    // reaches it, else its end nearest to the line.
    const room = s.hi - s.lo > TRACK ? { lo: s.lo + TRACK / 2, hi: s.hi - TRACK / 2 } : { lo: (s.lo + s.hi) / 2, hi: (s.lo + s.hi) / 2 };
    const at = Math.min(Math.max(y, room.lo), room.hi);
    if (cost(at) < best.cost - 0.5 || (Math.abs(cost(at) - best.cost) <= 0.5 && Math.abs(at - y) < Math.abs(best.at - y))) best = { cost: cost(at), at, span: s };
  }
  const key = `${Math.round(band.lo)}:${Math.round(band.hi)}`;
  const inWindow = (at: number) => at > window.lo && at < window.hi && !isBlocked(band.blocked, at);
  const shared = (gaps.get(key) ?? []).filter((u) => u.bundle === bundle && inWindow(u.at)).sort((p, q) => cost(p.at) - cost(q.at))[0];
  if (shared && cost(shared.at) <= best.cost + BUS_DETOUR) return { at: shared.at, key };
  const lane = { at: nearestFree(gaps.get(key) ?? [], best.at, { lo: best.span.lo + 1, hi: best.span.hi - 1 }, band), lo: band.lo, hi: band.hi, bundle };
  return { at: lane.at, key, lane };
}

/** A position near `ideal` within `room` for a segment covering `span` across, away from the
 *  segments of `list`; `ideal` (kept in the room) when there is no free one. */
function nearestFree(list: Lane[], ideal: number, room: Span, span: Span): number {
  const free = (at: number) => list.every((s) => Math.abs(s.at - at) >= TRACK - 0.5 || s.hi <= span.lo || s.lo >= span.hi);
  for (let k = 0; k < 64; k++) {
    const at = ideal + (k % 2 ? 1 : -1) * Math.ceil(k / 2) * TRACK;
    if (at >= room.lo && at <= room.hi && free(at)) return at;
  }
  return Math.min(Math.max(ideal, room.lo), room.hi);
}

/** An SVG path through `points`, with rounded corners. */
export function pathThrough(points: Point[], radius = 6): string {
  const p = points.filter((q, i) => i === 0 || q.x !== points[i - 1]!.x || q.y !== points[i - 1]!.y);
  if (p.length === 0) return "";
  let d = `M ${p[0]!.x} ${p[0]!.y}`;
  for (let i = 1; i < p.length - 1; i++) {
    const [a, b, c] = [p[i - 1]!, p[i]!, p[i + 1]!];
    const r = Math.min(radius, Math.hypot(b.x - a.x, b.y - a.y) / 2, Math.hypot(c.x - b.x, c.y - b.y) / 2);
    const towards = (q: Point, len: number) => {
      const l = Math.hypot(q.x - b.x, q.y - b.y) || 1;
      return { x: b.x + ((q.x - b.x) / l) * len, y: b.y + ((q.y - b.y) / l) * len };
    };
    const inner = towards(a, r);
    const outer = towards(c, r);
    d += ` L ${inner.x} ${inner.y} Q ${b.x} ${b.y} ${outer.x} ${outer.y}`;
  }
  const last = p[p.length - 1]!;
  return `${d} L ${last.x} ${last.y}`;
}
