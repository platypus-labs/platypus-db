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
  query?: string;
  /** See `ShownSort`; missing in a sort saved before it was kept. */
  original?: string | null;
}

/** `sql` as the viewer compares queries: trimmed, without a final `;`. */
export const queryKey = (sql: string) => sql.trim().replace(/;$/, "");

/** The range of a result's statement in the editor's `text`, when the text still holds it. */
export function statementRange(result: { sql: string; offset: number }, text: string): { from: number; to: number } | null {
  if (result.offset < 0) return null;
  const to = result.offset + result.sql.length;
  return text.slice(result.offset, to) === result.sql ? { from: result.offset, to } : null;
}

/** The sort of the result of `sql`: the viewer's, when it was written into that statement.
 *  Another statement, or the same one edited by hand, starts unsorted, with its own ORDER BY
 *  as the one to put back. */
export function sortOf(sort: ViewerSort | null | undefined, sql: string): ShownSort | null {
  if (sort?.query === undefined || sort.query !== queryKey(sql)) return null;
  return { column: sort.column, desc: sort.desc, original: sort.original ?? null };
}

/** The sort after a click on `column`'s header: ascending, then descending, then none (the
 *  statement's original ORDER BY, if it had one). */
export function nextSort(current: Sort | null, column: string): Sort | null {
  if (current?.column !== column) return { column, desc: false };
  return current.desc ? null : { column, desc: true };
}

/** A click on `column`'s header, from the shown statement's sort: the sort to write and the
 *  ORDER BY to put back when it is `null`. The first click reads the statement's own with
 *  `ownOrder`; the sort keeps it from then on. */
export async function clickSort(
  current: ShownSort | null,
  column: string,
  ownOrder: () => Promise<string | null>,
): Promise<{ sort: ShownSort | null; original: string | null }> {
  const next = nextSort(current, column);
  const original = current ? current.original : await ownOrder();
  return { sort: next && { ...next, original }, original };
}

/** `sort` as written into the statement `sql`. */
export const sortIn = (sort: ShownSort | null, sql: string): ViewerSort | null =>
  sort && { column: sort.column, desc: sort.desc, query: queryKey(sql), original: sort.original };

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
