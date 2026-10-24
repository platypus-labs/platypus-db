/**
 * How a grid shows its columns: their order, the hidden ones, the ones pinned to the left
 * and their widths. Columns are named, not numbered, so a table's layout still applies when
 * the query around it changes; a name the result does not have is ignored.
 */
import type { ColumnInfo } from "../api";

export interface ColumnLayout {
  /** Display order; columns not listed follow in result order. */
  order: string[];
  hidden: string[];
  /** Kept on the left while scrolling, in this order. */
  pinned: string[];
  widths: Record<string, number>;
}

export const EMPTY_LAYOUT: ColumnLayout = { order: [], hidden: [], pinned: [], widths: {} };

/** A layout applied to a result: the result columns shown, left to right. */
export interface ResolvedLayout {
  /** Result column of each display position. */
  view: number[];
  /** Display position of each result column; `-1` when hidden. */
  position: number[];
  /** How many of the first display positions are pinned. */
  pinned: number;
}

/** Result columns by name; a name shown twice maps to its columns in turn. */
function indexer(columns: ColumnInfo[]) {
  const byName = new Map<string, number[]>();
  columns.forEach((c, i) => byName.set(c.name, [...(byName.get(c.name) ?? []), i]));
  const used = new Set<number>();
  return (name: string) => {
    const i = byName.get(name)?.find((c) => !used.has(c));
    if (i !== undefined) used.add(i);
    return i;
  };
}

export function resolveLayout(columns: ColumnInfo[], layout: ColumnLayout): ResolvedLayout {
  const index = indexer(columns);
  const ordered: number[] = [];
  for (const name of layout.order) {
    const i = index(name);
    if (i !== undefined) ordered.push(i);
  }
  const listed = new Set(ordered);
  columns.forEach((_, i) => listed.has(i) || ordered.push(i));
  const hidden = new Set(layout.hidden);
  const pinned = new Set(layout.pinned);
  let visible = ordered.filter((i) => !hidden.has(columns[i]!.name));
  // Hiding every column would leave nothing to click to bring them back.
  if (visible.length === 0) visible = ordered;
  const byPin = (name: string) => layout.pinned.indexOf(name);
  const left = visible
    .filter((i) => pinned.has(columns[i]!.name))
    .sort((a, b) => byPin(columns[a]!.name) - byPin(columns[b]!.name));
  const view = [...left, ...visible.filter((i) => !pinned.has(columns[i]!.name))];
  const position = columns.map(() => -1);
  view.forEach((c, d) => (position[c] = d));
  return { view, position, pinned: left.length };
}

const names = (columns: ColumnInfo[], cols: number[]) => cols.map((c) => columns[c]!.name);

/** Moves the column at display position `from` to position `to` (counted after the move);
 *  crossing the edge of the pinned columns pins or unpins it. */
export function moveColumn(
  layout: ColumnLayout,
  columns: ColumnInfo[],
  resolved: ResolvedLayout,
  from: number,
  to: number,
): ColumnLayout {
  if (from === to) return layout;
  const view = [...resolved.view];
  const [moved] = view.splice(from, 1);
  view.splice(to, 0, moved!);
  const p = resolved.pinned;
  const pinned = from < p && to >= p ? p - 1 : from >= p && to < p ? p + 1 : p;
  // Hidden columns keep their place after the visible ones.
  const hidden = columns.map((_, i) => i).filter((i) => resolved.position[i] === -1);
  return {
    ...layout,
    order: names(columns, [...view, ...hidden]),
    pinned: names(columns, view.slice(0, pinned)),
  };
}

export function hideColumn(layout: ColumnLayout, name: string): ColumnLayout {
  return { ...layout, hidden: [...new Set([...layout.hidden, name])], pinned: layout.pinned.filter((n) => n !== name) };
}

export function pinColumn(layout: ColumnLayout, name: string, pinned: boolean): ColumnLayout {
  const rest = layout.pinned.filter((n) => n !== name);
  return { ...layout, pinned: pinned ? [...rest, name] : rest };
}

/** Column layouts of data viewer tables, kept in this browser profile. */
const KEY = "platypus.columns.v1";

function stored(): Record<string, ColumnLayout> {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) ?? "{}") as unknown;
    return parsed && typeof parsed === "object" ? (parsed as Record<string, ColumnLayout>) : {};
  } catch {
    return {};
  }
}

export function loadLayout(key: string): ColumnLayout {
  const layout = stored()[key];
  return layout ? { ...EMPTY_LAYOUT, ...layout } : EMPTY_LAYOUT;
}

export function saveLayout(key: string, layout: ColumnLayout) {
  try {
    const all = stored();
    const empty =
      !layout.order.length && !layout.hidden.length && !layout.pinned.length && !Object.keys(layout.widths).length;
    if (empty) delete all[key];
    else all[key] = layout;
    localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    // Storage can be unavailable or full; layouts are a convenience.
  }
}
