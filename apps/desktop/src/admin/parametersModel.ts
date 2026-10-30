// What the admin tab does with the server's configuration parameters: the search, the filter
// for those that differ from their defaults, and how a value reads. Types only from the app:
// the unit tests load this module without it.
import type { ServerParameter } from "../api";

/** Whether every word of `filter` is in the name, value, source or description of `p`,
 *  ignoring case. An empty filter matches all. */
export function parameterMatches(p: ServerParameter, filter: string): boolean {
  const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const text = [p.name, p.value, p.source, p.context, p.description].join("\u0000").toLowerCase();
  return words.every((w) => text.includes(w));
}

/** The parameters to show: those that match `filter`, and with `onlyChanged` those whose value
 *  differs from the default (or waits for a restart). */
export function shownParameters(list: ServerParameter[], filter: string, onlyChanged: boolean): ServerParameter[] {
  return list.filter((p) => (!onlyChanged || p.changed || p.pending_restart) && parameterMatches(p, filter));
}

/** How many of `list` differ from their defaults. */
export const changedCount = (list: ServerParameter[]) => list.filter((p) => p.changed).length;

/** The value with its unit: `16384 8kB`; empty when the server gave none. */
export function valueText(value: string | null, unit: string | null): string {
  if (value === null) return "";
  return unit && value !== "" ? `${value} ${unit}` : value;
}

/** Whether any parameter of `list` tells its default: without, the filter has nothing to
 *  compare and the tab says so. */
export const tellsDefaults = (list: ServerParameter[]) => list.some((p) => p.default !== null);
