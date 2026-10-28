import type { Explain, PlanNode } from "../api";

/** A step of either plan, side by side: the same step in both, or one only. */
export interface Pair {
  depth: number;
  before: PlanNode | null;
  after: PlanNode | null;
}

/** A step's operation and, when it has one, its target: `Seq Scan on orders o`. */
export const stepName = (n: PlanNode) => (n.detail ? `${n.operation} ${n.detail}` : n.operation);

const key = (n: PlanNode) => `${n.operation}\u0000${n.detail ?? ""}`;

/** The longest run of `a` and `b` with the same keys, as pairs of indexes. */
function common(a: PlanNode[], b: PlanNode[]): [number, number][] {
  const len = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0));
  for (let i = a.length - 1; i >= 0; i--)
    for (let j = b.length - 1; j >= 0; j--)
      len[i]![j] = key(a[i]!) === key(b[j]!) ? len[i + 1]![j + 1]! + 1 : Math.max(len[i + 1]![j]!, len[i]![j + 1]!);
  const out: [number, number][] = [];
  for (let i = 0, j = 0; i < a.length && j < b.length; ) {
    if (key(a[i]!) === key(b[j]!)) {
      out.push([i, j]);
      i++;
      j++;
    } else if (len[i + 1]![j]! >= len[i]![j + 1]!) i++;
    else j++;
  }
  return out;
}

/** The steps of two plans lined up: children with the same operation and target match in
 *  order; between them, the steps of one plan stand beside those of the other (a `Seq Scan`
 *  replaced by an `Index Scan`), and the rest alone. */
export function alignPlans(before: PlanNode, after: PlanNode): Pair[] {
  const out: Pair[] = [];
  const walk = (a: PlanNode | null, b: PlanNode | null, depth: number) => {
    out.push({ depth, before: a, after: b });
    const as = a?.children ?? [];
    const bs = b?.children ?? [];
    let i = 0;
    let j = 0;
    for (const [mi, mj] of [...common(as, bs), [as.length, bs.length] as [number, number]]) {
      // The steps between two matches go side by side.
      while (i < mi || j < mj) {
        walk(i < mi ? as[i]! : null, j < mj ? bs[j]! : null, depth + 1);
        if (i < mi) i++;
        if (j < mj) j++;
      }
      if (mi < as.length) walk(as[mi]!, bs[mj]!, depth + 1);
      i = mi + 1;
      j = mj + 1;
    }
  };
  walk(before, after, 0);
  return out;
}

/** What a plan compares on: its time over all loops when it ran, else the planner's cost. */
export type Measure = "time" | "cost";

export const measureOf = (a: Explain, b: Explain): Measure => (a.analyzed && b.analyzed ? "time" : "cost");

/** A step's own share of the measure: its own time over all loops, or its own cost. */
export const own = (n: PlanNode | null, m: Measure) => (n === null ? null : m === "time" ? n.self_ms : n.self_cost);

/** The whole plan's measure: the top step's time over all loops, or its cost. */
export function total(e: Explain, m: Measure): number | null {
  if (m === "cost") return e.root.cost;
  return e.root.actual_ms === null ? null : e.root.actual_ms * (e.root.loops ?? 1);
}

/** The change from `before` to `after` as a fraction of `before` (`-0.5` is half);
 *  `null` when either is missing or `before` is zero. */
export function change(before: number | null, after: number | null): number | null {
  if (before === null || after === null || before === 0) return null;
  return (after - before) / before;
}
