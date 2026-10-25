/**
 * The local notes and tags of each connection (`catalog.db`), loaded once per connection and
 * shared by the navigator, the object tab, the grid and the editor's completion.
 */
import { useSyncExternalStore } from "react";
import { api, type CatalogEntry, type CatalogKey, toCommandError } from "../api";
import { workbench } from "../workbench";

const loaded = new Map<string, Map<string, CatalogEntry>>();
const loading = new Map<string, Promise<void>>();
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

function load(connectionId: string): Promise<void> {
  const running = loading.get(connectionId);
  if (running) return running;
  if (loaded.has(connectionId)) return Promise.resolve();
  const p = api.catalogEntries(connectionId).then(
    (entries) => {
      loaded.set(connectionId, new Map(entries.map((e) => [id(e.key), e])));
    },
    (e: unknown) => {
      // The app works without notes; say so once.
      loaded.set(connectionId, new Map());
      if (!warned) {
        warned = true;
        workbench().showError(`Notes are unavailable: ${toCommandError(e).message}`);
      }
    },
  ).then(() => {
    loading.delete(connectionId);
    changed();
  });
  loading.set(connectionId, p);
  return p;
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
  await load(connectionId);
  const saved = await api.saveCatalogEntry(connectionId, key, note, tags);
  const entries = loaded.get(connectionId) ?? new Map<string, CatalogEntry>();
  if (saved.note || saved.tags.length) entries.set(id(key), saved);
  else entries.delete(id(key));
  loaded.set(connectionId, entries);
  changed();
}

export async function renameCatalog(connectionId: string, from: CatalogKey, to: CatalogKey) {
  await api.renameCatalogKey(connectionId, from, to);
  await loading.get(connectionId);
  const entries = await api.catalogEntries(connectionId);
  loaded.set(connectionId, new Map(entries.map((e) => [id(e.key), e])));
  changed();
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
    entry?.note && `Note (only on this computer): ${entry.note}`,
    entry?.tags.length && entry.tags.map((t) => `#${t}`).join(" "),
  ].filter(Boolean);
  return parts.length ? parts.join("\n") : undefined;
}
