// The text side of the catalog: the navigator's `#tag` filter and the tooltip of an annotated
// object. Pure, so it can be tested without the backend.
import type { CatalogEntry } from "../api";

/**
 * `#pii orders` → tags `["pii"]`, text `orders`. A `#` alone, or inside a word, is plain text.
 * The text keeps its spacing. With `known`, a `#word` that is not a known tag stays in the text,
 * so `#temp` (SQL Server temp tables) can be searched by name.
 */
export function parseFilter(text: string, known?: (tag: string) => boolean): { text: string; tags: string[] } {
  const tags: string[] = [];
  const rest = text.replace(/(^|\s)(#+([^\s#]\S*))/g, (all: string, _space: string, _word: string, tag: string) => {
    const lower = tag.toLowerCase();
    if (known && !known(lower)) return all;
    tags.push(lower);
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

/** What precedes the comment and the note in a tooltip, in the interface language. */
export interface AnnotationLabels {
  comment: string;
  note: string;
}

const ENGLISH: AnnotationLabels = { comment: "Comment (database)", note: "Note (only on this computer)" };

export interface AnnotationOptions {
  /** All of a long note, for a panel that has the room. */
  full?: boolean;
  labels?: AnnotationLabels;
}

/**
 * The tooltip of an annotated object or column: database comment, note, tags. A tooltip shows
 * the first line of a long note, unless `full`.
 */
export function annotation(
  entry: CatalogEntry | undefined,
  comment?: string | null,
  { full = false, labels = ENGLISH }: AnnotationOptions = {},
): string | undefined {
  const parts = [
    comment && `${labels.comment}: ${comment}`,
    entry?.note && `${labels.note}: ${full ? entry.note : firstLine(entry.note)}`,
    entry?.tags.length && entry.tags.map((t) => `#${t}`).join(" "),
  ].filter(Boolean);
  return parts.length ? parts.join("\n") : undefined;
}
