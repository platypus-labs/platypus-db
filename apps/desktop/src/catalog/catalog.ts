/**
 * The local notes and tags of each connection (`catalog.db`), loaded once per connection and
 * shared by the navigator, the object tab, the grid and the editor's completion.
 */
import { useSyncExternalStore } from "react";
import { api, type CatalogEntry, type CatalogKey, toCommandError } from "../api";
import { workbench } from "../workbench";

const loaded = new Map<string, Map<string, CatalogEntry>>();
const loading = new Set<string>();
const listeners = new Set<() => void>();
let version = 0;
let warned = false;

const id = (k: CatalogKey) => [k.database, k.schema, k.object, k.column].join("\u0000");

function changed() {
  version++;
  for (const l of listeners) l();
}

export function catalogKey(database: string | null | undefined, schema: string, object: string, column = ""): CatalogKey {
  return { database: database ?? "", schema, object, column };
}

function load(connectionId: string) {
  if (loaded.has(connectionId) || loading.has(connectionId)) return;
  loading.add(connectionId);
  api.catalogEntries(connectionId).then(
    (entries) => {
      loaded.set(connectionId, new Map(entries.map((e) => [id(e.key), e])));
      loading.delete(connectionId);
      changed();
    },
    (e: unknown) => {
      // The app works without notes; say so once.
      loaded.set(connectionId, new Map());
      loading.delete(connectionId);
      if (!warned) {
        warned = true;
        workbench().showError(`Notes are unavailable: ${toCommandError(e).message}`);
      }
    },
  );
}

/** The note and tags of `key`; `undefined` until loaded or when there are none. */
export function catalogEntry(connectionId: string, key: CatalogKey): CatalogEntry | undefined {
  const entries = loaded.get(connectionId);
  if (!entries) {
    load(connectionId);
    return undefined;
  }
  return entries.get(id(key));
}

export async function saveCatalog(connectionId: string, key: CatalogKey, note: string, tags: string[]) {
  const saved = await api.saveCatalogEntry(connectionId, key, note, tags);
  const entries = loaded.get(connectionId) ?? new Map<string, CatalogEntry>();
  if (saved.note || saved.tags.length) entries.set(id(key), saved);
  else entries.delete(id(key));
  loaded.set(connectionId, entries);
  changed();
}

export async function renameCatalog(connectionId: string, from: CatalogKey, to: CatalogKey) {
  await api.renameCatalogKey(connectionId, from, to);
  loaded.delete(connectionId);
  load(connectionId);
}

/** Re-renders the caller whenever notes are loaded or saved. */
export function useCatalogVersion(): number {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => version,
  );
}

/** `#pii orders` → tags `["pii"]`, text `orders`. */
export function parseFilter(text: string): { text: string; tags: string[] } {
  const words = text.trim().split(/\s+/).filter(Boolean);
  return {
    tags: words.filter((w) => w.startsWith("#") && w.length > 1).map((w) => w.slice(1).toLowerCase()),
    text: words.filter((w) => !w.startsWith("#")).join(" "),
  };
}

/** The tooltip of an annotated object or column: database comment, note, tags. */
export function annotation(entry: CatalogEntry | undefined, comment?: string | null): string | undefined {
  const parts = [
    comment && `Comment (database): ${comment}`,
    entry?.note && `Note: ${entry.note}`,
    entry?.tags.length && entry.tags.map((t) => `#${t}`).join(" "),
  ].filter(Boolean);
  return parts.length ? parts.join("\n") : undefined;
}
