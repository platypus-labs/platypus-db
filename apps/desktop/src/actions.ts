/**
 * Every user command in one registry: context menus, keyboard shortcuts and (later) the
 * command palette read from here. A feature adds commands by registering actions for a
 * scope instead of editing the component that shows the menu.
 *
 * A scope says where an action applies and what it receives. New scopes are added by
 * augmenting `ActionScopes` from the feature's own module:
 *
 *   declare module "../actions" {
 *     interface ActionScopes { "erd.table": { table: string } }
 *   }
 */
import type { ObjectMeta, SavedConnection, SchemaMeta } from "./api";
import type { GridActionContext } from "./grid/DataGrid";

export interface ActionScopes {
  /** Anywhere in the window; shortcuts are captured before the editor sees them. */
  global: undefined;
  "navigator.connection": { connection: SavedConnection };
  /** A database of the server; `database` is `null` for the saved one, `name` is its name. */
  "navigator.database": { connection: SavedConnection; database: string | null; name: string };
  /** `database`: `null` for the connection's saved database, else another one of the server. */
  "navigator.schema": { connection: SavedConnection; database: string | null; schema: SchemaMeta };
  /** A table, view, routine, trigger, ...; `target` is its quoted, qualified name. */
  "navigator.object": {
    connection: SavedConnection;
    database: string | null;
    schema: SchemaMeta;
    object: ObjectMeta;
    target: string;
  };
  "navigator.column": {
    connection: SavedConnection;
    database: string | null;
    schema: SchemaMeta;
    object: ObjectMeta;
    column: string;
  };
  /** A result grid with focus or a context menu. */
  grid: GridActionContext;
}

export type ActionScope = keyof ActionScopes;

export interface Action<S extends ActionScope = ActionScope> {
  /** Unique, `area.verb`, e.g. `grid.copy`. */
  id: string;
  scope: S;
  title: string;
  /** `Mod+Shift+T`: `Mod` is Ctrl, or Cmd on macOS. Several for alternatives. */
  shortcut?: string | string[];
  /** Menu section; sections are separated by a line, in order of first appearance. */
  group?: string;
  /** Position within the scope's menu, lowest first; default 100. */
  order?: number;
  /** Hidden from menus and shortcuts when it returns `false`. */
  when?: (arg: ActionScopes[S]) => boolean;
  /** Shortcut only, never listed in menus. */
  hidden?: boolean;
  run: (arg: ActionScopes[S]) => void | Promise<void>;
}

const registry = new Map<string, Action>();
const listeners = new Set<() => void>();

/** Adds or replaces (by id) an action; returns a function that removes it. */
export function registerAction<S extends ActionScope>(action: Action<S>): () => void {
  registry.set(action.id, action as unknown as Action);
  listeners.forEach((l) => l());
  return () => {
    if (registry.get(action.id) === (action as unknown as Action)) {
      registry.delete(action.id);
      listeners.forEach((l) => l());
    }
  };
}

/** Called whenever actions are added or removed, e.g. by the command palette. */
export function onActionsChanged(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Actions of a scope that apply to `arg`, in menu order. */
export function actionsFor<S extends ActionScope>(scope: S, arg: ActionScopes[S]): Action<S>[] {
  return ([...registry.values()] as unknown as Action<S>[])
    .filter((a) => a.scope === scope && (a.when?.(arg) ?? true))
    .sort((a, b) => (a.order ?? 100) - (b.order ?? 100));
}

/** Every registered action, for listings such as the command palette. */
export function allActions(): Action[] {
  return [...registry.values()];
}

const isMac = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

function matches(shortcut: string, e: KeyboardEvent): boolean {
  const parts = shortcut.split("+");
  const key = parts.pop()!.toLowerCase();
  const mods = new Set(parts.map((p) => p.toLowerCase()));
  const mod = isMac ? e.metaKey : e.ctrlKey;
  return (
    e.key.toLowerCase() === key &&
    mod === mods.has("mod") &&
    e.shiftKey === mods.has("shift") &&
    e.altKey === mods.has("alt") &&
    (isMac ? e.ctrlKey === mods.has("ctrl") : true)
  );
}

/** The action of `scope` whose shortcut is `e`, if any applies to `arg`. */
export function actionForKey<S extends ActionScope>(
  scope: S,
  arg: ActionScopes[S],
  e: KeyboardEvent,
): Action<S> | undefined {
  return actionsFor(scope, arg).find((a) => [a.shortcut ?? []].flat().some((s) => matches(s, e)));
}

/** Runs the matching action and swallows the event; `true` if one ran. */
export function dispatchKey<S extends ActionScope>(
  scope: S,
  arg: ActionScopes[S],
  e: KeyboardEvent,
  onError: (e: unknown) => void,
): boolean {
  const action = actionForKey(scope, arg, e);
  if (!action) return false;
  e.preventDefault();
  e.stopPropagation();
  runAction(action, arg, onError);
  return true;
}

export function runAction<S extends ActionScope>(
  action: Action<S>,
  arg: ActionScopes[S],
  onError: (e: unknown) => void,
) {
  try {
    void Promise.resolve(action.run(arg)).catch(onError);
  } catch (e) {
    onError(e);
  }
}

/** The first shortcut as shown in menus: `Ctrl+Shift+T`, or `⌘⇧T` on macOS. */
export function shortcutLabel(action: Action): string | undefined {
  const first = [action.shortcut ?? []].flat()[0];
  if (!first) return undefined;
  if (!isMac) return first.replace("Mod", "Ctrl");
  const symbols: Record<string, string> = { Mod: "⌘", Shift: "⇧", Alt: "⌥", Ctrl: "⌃" };
  return first
    .split("+")
    .map((p) => symbols[p] ?? p.toUpperCase())
    .join("");
}
