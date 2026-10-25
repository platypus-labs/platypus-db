import type { WheelEvent } from "react";

/** Scrolls a bar of tabs sideways with the wheel: it has nothing to scroll up or down. */
export function scrollSideways(e: WheelEvent<HTMLElement>) {
  if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) e.currentTarget.scrollLeft += e.deltaY;
}
