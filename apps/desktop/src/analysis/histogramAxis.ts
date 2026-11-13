/** Heights of a histogram's bars and its y axis (pure, so it is unit-tested). */
interface Bin {
  from: string;
  to: string;
  count: number;
}

/** The largest bar's count: the top of the y axis (at least 1, so an empty list still scales). */
export const yMax = (bins: Bin[]) => Math.max(1, ...bins.map((b) => b.count));

/** The bar's height as a share of the y axis, from 0 to 1. */
export const barShare = (b: Bin, max: number) => (max > 0 ? Math.min(1, Math.max(0, b.count / max)) : 0);
