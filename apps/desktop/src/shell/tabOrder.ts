/** Moving a tab by dragging it in the bar. Pure, so it is tested without the bar. */

interface Ordered {
  key: number;
  pinned?: boolean;
}

/** `tabs` with the tab `dragged` moved before (or, with `after`, behind) the tab `over`.
 *  A pinned tab stays among the pinned ones and a loose tab among the loose ones: a drop
 *  across that line, on itself, or on an unknown tab changes nothing (the same array). */
export function moveTab<T extends Ordered>(tabs: T[], dragged: number, over: number, after: boolean): T[] {
  const from = tabs.findIndex((t) => t.key === dragged);
  const target = tabs.findIndex((t) => t.key === over);
  const moved = tabs[from];
  if (!moved || !tabs[target] || from === target) return tabs;
  if (Boolean(moved.pinned) !== Boolean(tabs[target]!.pinned)) return tabs;
  const rest = tabs.filter((t) => t.key !== dragged);
  const at = rest.findIndex((t) => t.key === over) + (after ? 1 : 0);
  if (at === from) return tabs;
  return [...rest.slice(0, at), moved, ...rest.slice(at)];
}

/** The tab one step `step` (-1, +1) from `key` in the bar's order, for the keyboard
 *  alternative (Alt+Shift+Left/Right); `null` at an end. */
export function neighbour<T extends Ordered>(shown: T[], key: number, step: -1 | 1): T | null {
  const at = shown.findIndex((t) => t.key === key);
  return at < 0 ? null : (shown[at + step] ?? null);
}
