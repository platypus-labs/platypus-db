// What the schema history tab works out: which two states are compared, and the changes
// grouped by table. Types only from the app: the unit tests load this module without it.
import type { SchemaChange, SchemaSnapshot } from "../api";

/** The live schema, as a state to compare. */
export const NOW = "now";

/** What the user ticked: a snapshot's id, or `NOW`. */
export type Pick = number | typeof NOW;

/** `picks` with `pick` toggled; a third pick drops the one chosen first, so two stay at most. */
export function toggled(picks: Pick[], pick: Pick): Pick[] {
  if (picks.includes(pick)) return picks.filter((p) => p !== pick);
  return [...picks, pick].slice(-2);
}

/** The two states to compare as (older, newer), or `null` until two are picked. The schema
 *  now is always the newer one; of two snapshots the older is the one taken first. */
export function pair(picks: Pick[]): [number, Pick] | null {
  if (picks.length !== 2) return null;
  const snapshots = picks.filter((p): p is number => p !== NOW);
  if (snapshots.length === 2) return [Math.min(...snapshots), Math.max(...snapshots)];
  return snapshots.length === 1 ? [snapshots[0]!, NOW] : null;
}

/** The changes of one table. */
export interface TableChanges {
  table: string;
  changes: SchemaChange[];
}

/** `changes` grouped by table, in the order they come (they arrive by table). */
export function groupChanges(changes: SchemaChange[]): TableChanges[] {
  const groups: TableChanges[] = [];
  for (const change of changes) {
    const last = groups[groups.length - 1];
    if (last?.table === change.table) last.changes.push(change);
    else groups.push({ table: change.table, changes: [change] });
  }
  return groups;
}

/** The snapshots without the ids that no longer exist among `snapshots`. */
export function stillThere(picks: Pick[], snapshots: SchemaSnapshot[]): Pick[] {
  return picks.filter((p) => p === NOW || snapshots.some((s) => s.id === p));
}
