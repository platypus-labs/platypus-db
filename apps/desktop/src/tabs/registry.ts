/**
 * Kinds of workspace tab. The app shell only knows `TabBase`; each kind (SQL editor,
 * object editor, ER diagram, ...) registers how it renders, what it persists and its
 * label, so a feature adds a tab kind without touching `App.tsx`.
 *
 * A kind declares its model by augmenting `TabKinds` in its own module:
 *
 *   declare module "../tabs/registry" {
 *     interface TabKinds { erd: ErdTabModel }
 *   }
 *
 * and registers itself in a module imported by `tabs/index.ts`.
 */
import type { ComponentType } from "react";
import type { CompletionCatalog, SavedConnection } from "../api";

export interface TabKinds {}

export type TabKindName = keyof TabKinds & string;
export type TabModel = TabKinds[TabKindName];

/** What every tab has, whatever its kind. */
export interface TabBase {
  key: number;
  kind: TabKindName;
  connectionId: string;
  /** Shown before the connection name, e.g. the table opened from the navigator. */
  title?: string | null;
  /** Uncommitted changes are pending: closing asks first. Not persisted. */
  txOpen?: boolean;
}

type OwnFields<T> = Omit<T, "key" | "kind" | "connectionId">;

export type TabInit<K extends TabKindName> = Partial<OwnFields<TabKinds[K]>>;

export interface TabProps<T> {
  tab: T;
  connection: SavedConnection;
  active: boolean;
  /** Opens the tab's own session on first use; `null` if the user cancelled. */
  ensureSession: () => Promise<number | null>;
  /** The connection's navigator session, for side queries such as row counts. */
  sideSession: () => Promise<number | null>;
  catalog: CompletionCatalog | null;
  /** Asks for the autocompletion catalog; `force` reloads it after schema changes. */
  onNeedCatalog: (force: boolean) => void;
  onChange: (patch: Partial<T>) => void;
  onError: (message: string) => void;
}

export interface TabKind<K extends TabKindName> {
  kind: K;
  component: ComponentType<TabProps<TabKinds[K]>>;
  /** Fills the kind's fields from what the opener passed. */
  create: (init: TabInit<K>) => OwnFields<TabKinds[K]>;
  /** Tab label before the connection name. */
  label: (tab: TabKinds[K]) => string;
  /** JSON-safe state for the next start; `null` drops the tab. */
  persist: (tab: TabKinds[K]) => Record<string, unknown> | null;
  /** Inverse of `persist`, given what was saved (possibly by an older version). */
  restore: (saved: Record<string, unknown>) => TabInit<K>;
}

const kinds = new Map<string, TabKind<TabKindName>>();

export function registerTabKind<K extends TabKindName>(def: TabKind<K>) {
  kinds.set(def.kind, def as unknown as TabKind<TabKindName>);
}

export function tabKind(kind: string): TabKind<TabKindName> | undefined {
  return kinds.get(kind);
}

let nextKey = 1;

export function createTab<K extends TabKindName>(kind: K, connectionId: string, init: TabInit<K> = {}): TabModel {
  const def = tabKind(kind);
  if (!def) throw new Error(`unknown tab kind ${kind}`);
  return { ...def.create(init as TabInit<TabKindName>), key: nextKey++, kind, connectionId } as TabModel;
}
