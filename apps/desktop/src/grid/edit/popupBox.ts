/** Where a popup editor goes over a cell. */

interface Size {
  width: number;
  height: number;
}

export interface Box extends Size {
  left: number;
  top: number;
}

/** Room left between the popup and the grid's edges. */
const MARGIN = 4;

/**
 * The popup's box: at the cell, moved back inside the grid, and shrunk to the grid when the
 * grid is smaller than `size` (never below `min`), so that its actions stay in view.
 */
export function popupBox(cell: { left: number; top: number }, bounds: Size, size: Size, min: Size): Box {
  const fit = (wanted: number, room: number, least: number) => Math.max(least, Math.min(wanted, room - 2 * MARGIN));
  const width = fit(size.width, bounds.width, min.width);
  const height = fit(size.height, bounds.height, min.height);
  return {
    left: Math.max(0, Math.min(cell.left, bounds.width - width - MARGIN)),
    top: Math.max(0, Math.min(cell.top, bounds.height - height - MARGIN)),
    width,
    height,
  };
}
