// The text side of the catalog: the navigator's `#tag` filter and the tooltip of an annotated
// object. Pure, so it can be tested without the backend.
import type { CatalogEntry } from "../api";

/**
 * `#pii orders` → tags `["pii"]`, text `orders`. A `#` alone, or inside a word, is plain text.
 * The text keeps its spacing.
 */
export function parseFilter(text: string): { text: string; tags: string[] } {
  const tags: string[] = [];
  const rest = text.replace(/(^|\s)#+([^\s#]\S*)/g, (_, _space: string, tag: string) => {
    tags.push(tag.toLowerCase());
    return "";
  });
  return { tags, text: rest.trim() };
}

/** `tags` plus the comma-separated ones typed in `text` (`#Two Words, pii`), sorted, once each. */
export function addTags(tags: string[], text: string): string[] {
  const typed = text.split(",").map((t) => t.trim().replace(/^#+/, "").trim().split(/\s+/).join("-").toLowerCase());
  return [...new Set([...tags, ...typed.filter(Boolean)])].sort();
}

/** The first line of `note`, with an ellipsis when more follows. */
function firstLine(note: string): string {
  const [first = "", ...more] = note.split(/\r?\n/);
  return more.some((line) => line.trim()) ? `${first.trimEnd()}…` : first.trimEnd();
}

/**
 * The tooltip of an annotated object or column: database comment, note, tags. A tooltip shows
 * the first line of a long note; `full` keeps all of it, for a panel that has the room.
 */
export function annotation(
  entry: CatalogEntry | undefined,
  comment?: string | null,
  { full = false }: { full?: boolean } = {},
): string | undefined {
  const parts = [
    comment && `Comment (database): ${comment}`,
    entry?.note && `Note (only on this computer): ${full ? entry.note : firstLine(entry.note)}`,
    entry?.tags.length && entry.tags.map((t) => `#${t}`).join(" "),
  ].filter(Boolean);
  return parts.length ? parts.join("\n") : undefined;
}
