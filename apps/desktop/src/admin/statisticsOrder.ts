// What the admin tab tells of the server's statistics: the costliest statements in an order,
// and the tables and indexes worth a look. Types only from the app: the unit tests load this
// module without it.
import type { ObjectStatistics, ServerQuery } from "../api";

/** The orders of the costliest statements. */
export type QueryOrder = "total" | "mean" | "calls";

/** `queries` the costliest first by `order`. */
export function sortQueries(queries: ServerQuery[], order: QueryOrder): ServerQuery[] {
  const key = (q: ServerQuery) => (order === "total" ? q.total_seconds : order === "mean" ? q.mean_seconds : q.calls);
  return [...queries].sort((a, b) => key(b) - key(a));
}

/** Whether `filter` is in the statement, the database or the user of `query`, ignoring case. */
export function queryMatches(query: ServerQuery, filter: string): boolean {
  const needle = filter.trim().toLowerCase();
  return !needle || [query.query, query.database, query.user].some((f) => f?.toLowerCase().includes(needle));
}

/** Why a table or an index may want a look. */
export type Issue = "unused" | "full_scans" | "dead_rows" | "free_space" | "never_analyzed";

/** Tables smaller than this are read whole at no cost: their full scans are not a concern. */
const MANY_ROWS = 10_000;

/** The issues of `o`. `analyzeKnown`: the server tells when tables were analyzed, so one
 *  with no date never was (MySQL does not tell). */
export function issues(o: ObjectStatistics, analyzeKnown: boolean): Issue[] {
  const out: Issue[] = [];
  if (o.kind === "index") {
    // A unique index keeps its constraint even when no read uses it.
    if (!o.unique && o.index_scans === 0) out.push("unused");
    return out;
  }
  if ((o.rows ?? 0) >= MANY_ROWS && (o.full_scans ?? 0) > (o.index_scans ?? 0)) out.push("full_scans");
  if (deadShare(o) >= 0.2 && (o.dead_rows ?? 0) >= 1000) out.push("dead_rows");
  if (o.free_bytes !== null && o.size_bytes && o.free_bytes >= 10 * 1024 * 1024 && o.free_bytes >= 0.2 * o.size_bytes)
    out.push("free_space");
  if (analyzeKnown && o.last_analyze === null && (o.rows ?? 0) > 0) out.push("never_analyzed");
  return out;
}

/** The share of `o`'s rows that are dead, 0 to 1. */
export function deadShare(o: ObjectStatistics): number {
  const dead = o.dead_rows ?? 0;
  const all = dead + (o.rows ?? 0);
  return all === 0 ? 0 : dead / all;
}

/** Whether the server tells when its tables were analyzed: some table has a date. */
export const analyzeKnown = (objects: ObjectStatistics[]) => objects.some((o) => o.last_analyze !== null);

/** The table `o` belongs to: itself, or an index's. */
export const tableOf = (o: ObjectStatistics) => o.table ?? o.name;

/** Whether `filter` is in the schema, name or table of `o`, ignoring case. */
export function objectMatches(o: ObjectStatistics, filter: string): boolean {
  const needle = filter.trim().toLowerCase();
  return !needle || [o.schema, o.name, o.table].some((f) => f?.toLowerCase().includes(needle));
}

/** `objects` to show: those that match `filter`, and with `onlyIssues` those with an issue,
 *  each table with the indexes shown under it. A table shows when one of its indexes does. */
export function shownObjects(objects: ObjectStatistics[], filter: string, onlyIssues: boolean): ObjectStatistics[] {
  const known = analyzeKnown(objects);
  const wanted = (o: ObjectStatistics) => objectMatches(o, filter) && (!onlyIssues || issues(o, known).length > 0);
  const key = (o: ObjectStatistics) => `${o.schema}\u0000${tableOf(o)}`;
  const tables = new Set(objects.filter(wanted).map(key));
  return objects.filter((o) => wanted(o) || (o.kind === "table" && tables.has(key(o))));
}
