/**
 * What the user keeps per connection in the navigator: bookmarked objects and filters that
 * hide schemas or objects. Stored in the web view's local storage, like grid layouts.
 */
import { useSyncExternalStore } from "react";
import type { ObjectKind } from "../api";

export interface Bookmark {
  connectionId: string;
  database: string | null;
  schema: string;
  /** Whether `schema` was the session's current one: the name is then left unqualified. */
  currentSchema: boolean;
  name: string;
  kind: ObjectKind;
  parent: string | null;
}

/** Comma-separated name patterns to hide; `*` matches any text, case is ignored. */
interface NavigatorFilter {
  schemas: string;
  objects: string;
}

interface Prefs {
  bookmarks: Bookmark[];
  /** By connection id. */
  filters: Record<string, NavigatorFilter>;
  /** The folders of connections left closed; a folder starts open. */
  closedFolders: string[];
}

const KEY = "platypus.navigator";
const listeners = new Set<() => void>();
let prefs = read();

function read(): Prefs {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Prefs>;
    return {
      bookmarks: Array.isArray(saved.bookmarks) ? saved.bookmarks : [],
      filters: saved.filters && typeof saved.filters === "object" ? saved.filters : {},
      closedFolders: Array.isArray(saved.closedFolders) ? saved.closedFolders : [],
    };
  } catch {
    return { bookmarks: [], filters: {}, closedFolders: [] };
  }
}

function write(next: Prefs) {
  prefs = next;
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch (e) {
    console.warn("navigator preferences", e);
  }
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useNavigatorPrefs(): Prefs {
  return useSyncExternalStore(subscribe, () => prefs);
}

const same = (a: Bookmark, b: Bookmark) =>
  a.connectionId === b.connectionId &&
  a.database === b.database &&
  a.schema === b.schema &&
  a.name === b.name &&
  a.kind === b.kind &&
  a.parent === b.parent;

/** Remembers whether a folder of connections is closed, for the next start too. */
export function setFolderClosed(id: string, closed: boolean) {
  if (prefs.closedFolders.includes(id) === closed) return;
  const others = prefs.closedFolders.filter((f) => f !== id);
  write({ ...prefs, closedFolders: closed ? [...others, id] : others });
}

export const isBookmarked = (b: Bookmark) => prefs.bookmarks.some((x) => same(x, b));

export function toggleBookmark(b: Bookmark) {
  const bookmarks = isBookmarked(b) ? prefs.bookmarks.filter((x) => !same(x, b)) : [...prefs.bookmarks, b];
  write({ ...prefs, bookmarks });
}

/** Points the bookmarks of an object renamed from `from` to `to`, with the triggers that
 *  name it as their table. */
export function renameBookmarks(
  where: Pick<Bookmark, "connectionId" | "database" | "schema" | "kind">,
  from: string,
  to: string,
) {
  const inSchema = (b: Bookmark) =>
    b.connectionId === where.connectionId && b.database === where.database && b.schema === where.schema;
  let changed = false;
  const bookmarks = prefs.bookmarks.map((b) => {
    if (!inSchema(b)) return b;
    if (b.kind === where.kind && b.name === from) {
      changed = true;
      return { ...b, name: to };
    }
    if (b.kind === "trigger" && b.parent === from && where.kind === "table") {
      changed = true;
      return { ...b, parent: to };
    }
    return b;
  });
  if (changed) write({ ...prefs, bookmarks });
}

/** The bookmarks left once an object is dropped: its own, and its triggers when it is a
 *  table. */
export function withoutObject(
  bookmarks: Bookmark[],
  where: Pick<Bookmark, "connectionId" | "database" | "schema" | "kind">,
  name: string,
): Bookmark[] {
  return bookmarks.filter((b) => {
    if (b.connectionId !== where.connectionId || b.database !== where.database || b.schema !== where.schema) return true;
    if (b.kind === where.kind && b.name === name) return false;
    return !(b.kind === "trigger" && where.kind === "table" && b.parent === name);
  });
}

/** Removes the bookmarks of an object that was dropped. */
export function removeBookmarks(where: Pick<Bookmark, "connectionId" | "database" | "schema" | "kind">, name: string) {
  const bookmarks = withoutObject(prefs.bookmarks, where, name);
  if (bookmarks.length !== prefs.bookmarks.length) write({ ...prefs, bookmarks });
}

export const navigatorFilter =(connectionId: string): NavigatorFilter | undefined => prefs.filters[connectionId];

/** `null` or empty patterns remove the connection's filter. */
export function setNavigatorFilter(connectionId: string, filter: NavigatorFilter | null) {
  const filters = { ...prefs.filters };
  if (filter && (filter.schemas.trim() || filter.objects.trim())) filters[connectionId] = filter;
  else delete filters[connectionId];
  write({ ...prefs, filters });
}

/** Whether a name matches one of the comma-separated `patterns`, compiled once. */
export function nameMatcher(patterns: string | undefined): (name: string) => boolean {
  const globs = (patterns ?? "")
    .split(",")
    .map((p) => p.trim())
    .filter(Boolean)
    .map(glob);
  return (name) => globs.some((g) => g.test(name));
}

const glob = (pattern: string) =>
  new RegExp(`^${pattern.split("*").map((part) => part.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*")}$`, "i");
