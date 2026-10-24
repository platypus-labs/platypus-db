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

export function onNavigatorRefresh(listener: (detail: NavigatorRefresh) => void): () => void {
  const handler = (e: Event) => listener((e as CustomEvent<NavigatorRefresh>).detail);
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}
