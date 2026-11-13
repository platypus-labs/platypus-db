/** Where a submenu goes, relative to its anchor row, so it stays inside the window. */
interface Placement {
  left: number;
  top: number;
  /** Set when the submenu is taller than the window: it scrolls. */
  maxHeight?: number;
}

interface Box {
  left: number;
  top: number;
  width: number;
}

/** Opens to the right of the anchor, flips to the left at the right edge, shifts up at the
 *  bottom edge; taller than the window it gets a max-height and scrolls. */
export function submenuPlacement(anchor: Box, size: { width: number; height: number }, win: { width: number; height: number }): Placement {
  const margin = 4;
  const room = win.height - 2 * margin;
  const height = Math.min(size.height, room);
  const right = anchor.left + anchor.width - margin;
  const absLeft = right + size.width > win.width - margin ? Math.max(0, anchor.left + margin - size.width) : right;
  const absTop = Math.max(margin, Math.min(anchor.top - 5, win.height - height - margin));
  return { left: absLeft - anchor.left, top: absTop - anchor.top, ...(size.height > room && { maxHeight: room }) };
}
