/**
 * The local notes, tags and display columns of each connection (`catalog.db`), loaded once per
 * connection and shared by the navigator, the object tab, the grid and the editor's completion.
 */
import { useSyncExternalStore } from "react";
import { api, type CatalogEntry, type CatalogKey, type DisplayColumn, toCommandError } from "../api";
import { workbench } from "../workbench";
import { t } from "../i18n";
import { annotation as describe, type AnnotationOptions } from "./annotation";
import { keyedQueue } from "./queue";

export { parseFilter } from "./annotation";

/** The tooltip of an annotated object or column (`annotation.ts`), in the interface language. */
export function annotation(
  entry: CatalogEntry | undefined,
  comment?: string | null,
  options: Omit<AnnotationOptions, "labels"> = {},
): string | undefined {
  const labels = { comment: t("Comment (database)"), note: t("Note (only on this computer)") };
  return describe(entry, comment, { ...options, labels });
}

const loaded = new Map<string, Map<string, CatalogEntry>>();
/** Per connection: `database \0 schema \0 table` → the column that describes its rows. */
const displays = new Map<string, Map<string, string>>();
const loading = new Map<string, Promise<void>>();
/**
 * The writes to the catalog of a connection, one after the other. A refetch (after a rename)
 * replaces the cache with what it read, so a save that finished meanwhile would be lost from it.
 */
const writes = keyedQueue();
const listeners = new Set<() => void>();
let version = 0;
let warned = false;

const id = (k: CatalogKey) => [k.database, k.schema, k.object, k.column].join("\u0000");
const tableId = (database: string | null | undefined, schema: string, table: string) =>
  [database ?? "", schema, table].join("\u0000");

async function fetchCatalog(connectionId: string) {
  const [entries, columns] = await Promise.all([
    api.catalogEntries(connectionId),
    api.catalogDisplayColumns(connectionId),
  ]);
  store(connectionId, entries, columns);
}

function store(connectionId: string, entries: CatalogEntry[], columns: DisplayColumn[]) {
  loaded.set(connectionId, new Map(entries.map((e) => [id(e.key), e])));
  displays.set(connectionId, new Map(columns.map((d) => [tableId(d.key.database, d.key.schema, d.key.object), d.column])));
}

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
  const p = fetchCatalog(connectionId).catch((e: unknown) => {
    // The app works without notes; say so once.
    store(connectionId, [], []);
    if (!warned) {
      warned = true;
      workbench().showError(t("Notes are unavailable: {error}", { error: toCommandError(e).message }));
    }
  }).then(() => {
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

/**
 * What a save changes: the note, the tags from the ones stored when the save runs, or both.
 * What it leaves out keeps its stored value, so two saves in a row both count.
 */
export interface CatalogChange {
  note?: string;
  tags?: (stored: string[]) => string[];
}

export function saveCatalog(connectionId: string, key: CatalogKey, change: CatalogChange): Promise<void> {
  return writes.run(connectionId, async () => {
    await load(connectionId);
    const stored = loaded.get(connectionId)?.get(id(key));
    const note = change.note ?? stored?.note ?? "";
    const tags = change.tags ? change.tags(stored?.tags ?? []) : (stored?.tags ?? []);
    const saved = await api.saveCatalogEntry(connectionId, key, note, tags);
    const entries = loaded.get(connectionId) ?? new Map<string, CatalogEntry>();
    if (saved.note || saved.tags.length) entries.set(id(key), saved);
    else entries.delete(id(key));
    loaded.set(connectionId, entries);
    changed();
  });
}

export function renameCatalog(connectionId: string, from: CatalogKey, to: CatalogKey): Promise<void> {
  return writes.run(connectionId, async () => {
    await api.renameCatalogKey(connectionId, from, to);
    await loading.get(connectionId);
    try {
      await fetchCatalog(connectionId);
    } catch (e) {
      // Not the names from before the rename: the next read loads them again.
      loaded.delete(connectionId);
      displays.delete(connectionId);
      throw e;
    } finally {
      changed();
    }
  });
}

/** Resolves `true` when `write` went through; otherwise reports the error and resolves `false`. */
export const succeeded = (write: Promise<unknown>, report: (e: unknown) => void) =>
  write.then(
    () => true,
    (e: unknown) => {
      report(e);
      return false;
    },
  );

/** Drops what is cached for a deleted connection. */
export function forgetCatalog(connectionId: string) {
  loaded.delete(connectionId);
  displays.delete(connectionId);
  loading.delete(connectionId);
  writes.forget(connectionId);
}

/** The column picked to describe the rows of a table; `undefined` for the automatic choice. */
export function displayColumn(
  connectionId: string,
  database: string | null | undefined,
  schema: string,
  table: string,
): string | undefined {
  const columns = displays.get(connectionId);
  if (!columns) {
    load(connectionId);
    return undefined;
  }
  return columns.get(tableId(database, schema, table));
}

/** Picks the column that describes the rows of a table; `null` goes back to the automatic one. */
export function setDisplayColumn(
  connectionId: string,
  database: string | null | undefined,
  schema: string,
  table: string,
  column: string | null,
): Promise<void> {
  return writes.run(connectionId, async () => {
    await load(connectionId);
    await api.setCatalogDisplayColumn(connectionId, catalogKey(database, schema, table), column);
    const columns = displays.get(connectionId) ?? new Map<string, string>();
    if (column === null) columns.delete(tableId(database, schema, table));
    else columns.set(tableId(database, schema, table), column);
    displays.set(connectionId, columns);
    changed();
  });
}

/** `columns`, plus the picked one when the table no longer has it, so the select shows it. */
export const withPicked = (columns: string[], picked: string) =>
  picked && !columns.includes(picked) ? [...columns, picked] : columns;

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}

const currentVersion = () => version;

/** Re-renders the caller whenever notes are loaded or saved. */
export function useCatalogVersion(): number {
  return useSyncExternalStore(subscribe, currentVersion);
}
