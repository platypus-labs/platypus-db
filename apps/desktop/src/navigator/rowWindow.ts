// The rows of a long navigator tree that are rendered: those in view and `OVERSCAN` on each
// side. The window moves when the scroll passes a step of `STEP` rows, so scrolling within
// a step renders nothing: a tree of a few hundred rows is rendered whole, one of 10,000
// tables only around what is shown.

/** Height of a row: `.tree-row` in navigator.css. */
export const ROW_HEIGHT = 28;
const OVERSCAN = 200;
const STEP = 100;
/** Rows that fill the tallest view. */
const PAGE = 100;

/** The step of rows a tree scrolled to `scrollTop` is at. */
export const stepAt = (scrollTop: number) => Math.floor(scrollTop / ROW_HEIGHT / STEP);

/** The rows rendered at `step` of a tree of `count` rows: indexes `from` to `to`, excluded. */
export function rowWindow(step: number, count: number): { from: number; to: number } {
  return {
    from: Math.max(0, Math.min(step * STEP, count) - OVERSCAN),
    to: Math.min(count, step * STEP + PAGE + OVERSCAN),
  };
}

/** The scroll that brings row `index` into a view of `height` scrolled to `scrollTop`,
 *  moving it the least. */
export function scrollToRow(index: number, scrollTop: number, height: number): number {
  const top = index * ROW_HEIGHT;
  if (top < scrollTop) return top;
  return Math.max(scrollTop, top + ROW_HEIGHT - height);
}
