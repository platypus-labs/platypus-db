// The data viewer's header sort: the statement it belongs to and how a click moves it on.
// Plain logic, tested on its own (tests/viewerSort.test.mjs).
import type { Sort } from "../sqlNames";

/** The viewer's sort of a shown statement. */
export interface ShownSort extends Sort {
  /** The ORDER BY the statement had before the viewer's first sort, as the user wrote it: the
   *  click after descending puts it back. `null` when it had none. */
  original: string | null;
}

/** A sort the viewer wrote into a statement. */
export interface ViewerSort extends Sort {
  /** `queryKey` of the statement it was written into: it holds for that statement only. */
  query: string;
  /** See `ShownSort`; missing in a sort saved before it was kept. */
  original?: string | null;
}

/** How many statements' sorts a tab keeps, the latest ones. */
const KEPT_SORTS = 20;

/** `sql` as the viewer compares queries: trimmed, without a final `;`. */
export const queryKey = (sql: string) => sql.trim().replace(/;$/, "");

/** The range of a result's statement in the editor's `text`, when the text still holds it. */
export function statementRange(result: { sql: string; offset: number }, text: string): { from: number; to: number } | null {
  if (result.offset < 0) return null;
  const to = result.offset + result.sql.length;
  return text.slice(result.offset, to) === result.sql ? { from: result.offset, to } : null;
}

/** The sort of the result of `sql`: among a tab's `sorts`, the one written into that statement.
 *  Another statement, or the same one edited by hand, starts unsorted, with its own ORDER BY
 *  as the one to put back. */
export function sortOf(sorts: readonly ViewerSort[] | undefined, sql: string): ShownSort | null {
  const key = queryKey(sql);
  const sort = sorts?.find((s) => s.query === key);
  return sort ? { column: sort.column, desc: sort.desc, original: sort.original ?? null, ...at(sort.position) } : null;
}

const at = (position: number | undefined) => (position === undefined ? {} : { position });

/** A name the sort can write: one that is no expression's text (`?column?`, `COUNT(*)`), which
 *  the database would not read as the column, or as another column. */
const PLAIN_NAME = /^[\p{L}_][\p{L}\p{N}_$]*$/u;

/** The place (from 1) of result column `index` when the sort must name it by it: its name is
 *  shared with another column (two `id` of a join) or is no plain name. `undefined` when the
 *  name is enough. */
export function sortPosition(columns: readonly { name: string }[], index: number): number | undefined {
  const name = columns[index]?.name ?? "";
  const shared = columns.some((c, i) => i !== index && c.name.toLowerCase() === name.toLowerCase());
  return shared || !PLAIN_NAME.test(name) ? index + 1 : undefined;
}

/** The sort after a click on `column`'s header (at `position` when the name is not enough):
 *  ascending, then descending, then none (the statement's original ORDER BY, if it had one). */
export function nextSort(current: Sort | null, column: string, position?: number): Sort | null {
  if (current?.column !== column || current.position !== position) return { column, desc: false, ...at(position) };
  return current.desc ? null : { column, desc: true, ...at(position) };
}

/** A click on `column`'s header, from the shown statement's sort: the sort to write and the
 *  ORDER BY to put back when it is `null`. The first click reads the statement's own with
 *  `ownOrder`; the sort keeps it from then on. */
export async function clickSort(
  current: ShownSort | null,
  column: string,
  ownOrder: () => Promise<string | null>,
  position?: number,
): Promise<{ sort: ShownSort | null; original: string | null }> {
  const next = nextSort(current, column, position);
  const original = current ? current.original : await ownOrder();
  return { sort: next && { ...next, original }, original };
}

/** A tab's `sorts` once the statement `sql` has `sort`, or none: it replaces the one `sql` had.
 *  The others stay, so that each statement of a script keeps its own. */
export function withSort(sorts: readonly ViewerSort[] | undefined, sql: string, sort: ShownSort | null): ViewerSort[] {
  const query = queryKey(sql);
  const others = (sorts ?? []).filter((s) => s.query !== query);
  const all = sort
    ? [...others, { column: sort.column, desc: sort.desc, query, original: sort.original, ...at(sort.position) }]
    : others;
  return all.slice(-KEPT_SORTS);
}

/** The sorts a tab saved: its list, or the single sort saved before there was one; anything
 *  malformed is dropped. */
export function savedSorts(sorts: unknown, single: unknown): ViewerSort[] {
  const list: unknown[] = Array.isArray(sorts) ? sorts : single ? [single] : [];
  return list.filter((s): s is ViewerSort => {
    if (!s || typeof s !== "object") return false;
    const { column, desc, query, original, position } = s as Record<string, unknown>;
    return (
      typeof column === "string" &&
      typeof desc === "boolean" &&
      typeof query === "string" &&
      (original === undefined || original === null || typeof original === "string") &&
      (position === undefined || (typeof position === "number" && Number.isInteger(position) && position > 0))
    );
  });
}

/** The data viewer's own queries: `own` (the one it opened with, its table's plain query) and
 *  the rewrites of them by the header sort and the quick filters. Only these read exactly the
 *  viewer's table, so only their rows are edited as its rows; another query typed in the tab,
 *  or a rewrite of one, reads whatever it names. */
export function viewerQueries(...own: (string | null | undefined)[]) {
  const keys = new Set(own.filter((sql): sql is string => !!sql).map(queryKey));
  return {
    has: (sql: string) => keys.has(queryKey(sql)),
    /** Records `next`, written from `source`; true when it is one of the viewer's queries,
     *  which it is when `source` is. */
    rewritten(source: string, next: string): boolean {
      if (!keys.has(queryKey(source))) return false;
      keys.add(queryKey(next));
      return true;
    },
  };
}
