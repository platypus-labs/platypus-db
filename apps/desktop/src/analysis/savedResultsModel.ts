/** What the saved results' panel decides in the UI: the name a result is offered under, and
 *  which saved results a search shows. Pure: the unit tests import it. */

interface SavedLike {
  name: string;
  sql: string;
  connection: string;
}

/** Longest default name, in characters. */
export const NAME_LIMIT = 60;

/** The name offered for a result of `sql`: its first line that is not empty or a comment, cut
 *  with an ellipsis; `fallback` for a statement without text. */
export function defaultName(sql: string, fallback: string): string {
  const line = sql
    .split(/\r?\n/)
    .map((l) => l.trim())
    .find((l) => l !== "" && !l.startsWith("--"));
  if (!line) return fallback;
  const text = line.replace(/\s+/g, " ").replace(/;$/, "").trimEnd();
  const chars = [...text];
  return chars.length > NAME_LIMIT ? `${chars.slice(0, NAME_LIMIT - 1).join("").trimEnd()}…` : text;
}

/** The saved results that contain every word of `query` in their name, query or connection,
 *  whatever the case; all of them for an empty search. */
export function filterSaved<T extends SavedLike>(results: readonly T[], query: string): T[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...results];
  return results.filter((r) => {
    const haystack = `${r.name}\n${r.sql}\n${r.connection}`.toLowerCase();
    return words.every((w) => haystack.includes(w));
  });
}
