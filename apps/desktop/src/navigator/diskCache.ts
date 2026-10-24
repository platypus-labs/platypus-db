/**
 * The navigator's loaded metadata kept on disk (IndexedDB of the web view), one record per
 * connection: the tree shows at once on the next start, and offline. Failures are logged
 * and ignored: this is only a cache.
 */

/** Records not written for this long are dropped, e.g. those of deleted connections. */
const MAX_AGE_MS = 30 * 24 * 3600 * 1000;
const STORE = "connections";

interface Cached {
  id: string;
  savedAt: number;
  /** Navigator cache key → loaded value. */
  entries: [string, unknown][];
}

let opening: Promise<IDBDatabase> | null = null;

function db(): Promise<IDBDatabase> {
  opening ??= new Promise((resolve, reject) => {
    const request = indexedDB.open("platypus-metadata", 1);
    request.onupgradeneeded = () =>
      request.result.createObjectStore(STORE, { keyPath: "id" }).createIndex("savedAt", "savedAt");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  return opening;
}

async function run<T>(mode: IDBTransactionMode, use: (store: IDBObjectStore) => IDBRequest<T>): Promise<T | undefined> {
  try {
    const store = (await db()).transaction(STORE, mode).objectStore(STORE);
    return await new Promise<T>((resolve, reject) => {
      const request = use(store);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  } catch (e) {
    console.warn("metadata cache", e);
    return undefined;
  }
}

export async function loadCached(connectionId: string): Promise<[string, unknown][]> {
  const record = await run<Cached | undefined>("readonly", (s) => s.get(connectionId));
  return record && Date.now() - record.savedAt < MAX_AGE_MS ? record.entries : [];
}

export function saveCached(connectionId: string, entries: [string, unknown][]) {
  const record: Cached = { id: connectionId, savedAt: Date.now(), entries };
  void run("readwrite", (s) => s.put(record));
}

export function dropCached(connectionId: string) {
  void run("readwrite", (s) => s.delete(connectionId));
}

/** Drops the records older than `MAX_AGE_MS`; once per start is enough. */
export async function pruneCached() {
  const old = IDBKeyRange.upperBound(Date.now() - MAX_AGE_MS);
  const ids = await run<IDBValidKey[]>("readonly", (s) => s.index("savedAt").getAllKeys(old));
  for (const id of ids ?? []) dropCached(String(id));
}
