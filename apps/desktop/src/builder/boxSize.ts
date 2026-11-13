/** The default width of a table box on the canvas, and the smallest a box may be resized to. */
export const BOX = 200;
export const BOX_MIN_W = 140;
export const BOX_MIN_H = 80;

/** A box's own size; missing means the default width and a height that follows the columns. */
interface Sized {
  w?: number;
  h?: number;
}

export const boxWidth = (box: Sized) => box.w ?? BOX;

/** The size after a resize handle moved by (`dx`, `dy`) from `from` (the size when the drag began). */
export function resizedBox(from: { w: number; h: number }, dx: number, dy: number): { w: number; h: number } {
  return { w: Math.max(BOX_MIN_W, Math.round(from.w + dx)), h: Math.max(BOX_MIN_H, Math.round(from.h + dy)) };
}
