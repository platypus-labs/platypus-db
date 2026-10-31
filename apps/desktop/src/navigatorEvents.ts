/**
 * Asks the navigator (and whatever caches metadata, such as autocompletion) to reload a
 * connection, one of its databases or one schema, after something changed objects: DDL
 * run from an action, an object tab, the editor.
 */
const EVENT = "platypus:navigator-refresh";

export interface NavigatorRefresh {
  connectionId: string;
  /** `null` or missing: the saved database. */
  database?: string | null;
  /** Missing: every schema of the database. */
  schema?: string;
}

export function requestNavigatorRefresh(detail: NavigatorRefresh) {
  window.dispatchEvent(new CustomEvent<NavigatorRefresh>(EVENT, { detail }));
}

const REVEAL = "platypus:navigator-reveal";

/** Selects a connection's row in the navigator and gives it the focus, e.g. a new one. */
export function revealConnection(connectionId: string) {
  window.dispatchEvent(new CustomEvent<string>(REVEAL, { detail: connectionId }));
}

export function onRevealConnection(listener: (connectionId: string) => void): () => void {
  const handler = (e: Event) => listener((e as CustomEvent<string>).detail);
  window.addEventListener(REVEAL, handler);
  return () => window.removeEventListener(REVEAL, handler);
}

const DATA_AGAIN = "platypus:data-again";

/** "Open data" on a table whose data tab is open already: the tab, `tabKey`, reads its rows
 *  again when it shows none, its last read having failed (e.g. while the server was down). */
export function requestDataAgain(tabKey: number) {
  window.dispatchEvent(new CustomEvent<number>(DATA_AGAIN, { detail: tabKey }));
}

export function onDataAgain(listener: (tabKey: number) => void): () => void {
  const handler = (e: Event) => listener((e as CustomEvent<number>).detail);
  window.addEventListener(DATA_AGAIN, handler);
  return () => window.removeEventListener(DATA_AGAIN, handler);
}

export function onNavigatorRefresh(listener: (detail: NavigatorRefresh) => void): () => void {
  const handler = (e: Event) => listener((e as CustomEvent<NavigatorRefresh>).detail);
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}
