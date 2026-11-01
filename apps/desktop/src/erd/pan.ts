/** The scroll position after dragging the empty canvas from `from` to `to`: the view follows
 *  the pointer, so the scroll goes the other way. Never negative. */
export function pannedScroll(start: { left: number; top: number }, from: { x: number; y: number }, to: { x: number; y: number }) {
  return { left: Math.max(0, start.left - (to.x - from.x)), top: Math.max(0, start.top - (to.y - from.y)) };
}
