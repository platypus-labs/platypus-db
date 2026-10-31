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
import type { ObjectMeta, Project, SavedConnection, SchemaMeta } from "./api";
import { useSyncExternalStore } from "react";
import { getStored, setStored, useSettingsVersion } from "./settings";
import type { GridActionContext } from "./grid/DataGrid";
import { language } from "./i18n";
import { keyName, labelParts, matchesShortcut, splitShortcuts } from "./shortcutKeys";

export interface ActionScopes {
  /** Anywhere in the window; shortcuts are captured before the editor sees them. */
  global: undefined;
  /** The navigator's list of connections, away from any row; `project` is the active one. */
  "navigator.list": { project: Project | null };
  "navigator.connection": { connection: SavedConnection };
  /** A database of the server; `database` is `null` for the saved one, `name` is its name. */
  "navigator.database": { connection: SavedConnection; database: string | null; name: string };
  /** `database`: `null` for the connection's saved database, else another one of the server. */
  "navigator.schema": { connection: SavedConnection; database: string | null; schema: SchemaMeta };
  /** A table, view, routine, trigger, ...; `target` is its quoted, qualified name. */
  "navigator.object": NavigatorObject & {
    /** Every selected object, the clicked one included, when several are selected. */
    selection?: NavigatorObject[];
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

export interface NavigatorObject {
  connection: SavedConnection;
  database: string | null;
  schema: SchemaMeta;
  object: ObjectMeta;
  target: string;
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
  /** Listed but not run when it returns a reason (a `msg` text): menus and the palette show it
   *  greyed out with the reason, and its shortcut is swallowed. */
  disabled?: (arg: ActionScopes[S]) => string | null;
  /** Whether the item is on, for an action that switches something: shown as a check box in menus. */
  checked?: (arg: ActionScopes[S]) => boolean;
  /** Only an item of its menu: no shortcut can be set, so Settings › Keyboard shortcuts leaves it out. */
  menuOnly?: boolean;
  /** Shortcut only, never listed in menus. */
  hidden?: boolean;
  /** Also offered when several rows are selected; it gets them all in `arg.selection`. */
  multiple?: boolean;
  /** Listed in a submenu with this title, e.g. `Copy as`, at the place of its first action. */
  submenu?: string;
  run: (arg: ActionScopes[S]) => void | Promise<void>;
}

const registry = new Map<string, Action>();
const listeners = new Set<() => void>();
/** Bumped at every change of the registry, for `useActions`. */
let registryVersion = 0;

/** Adds or replaces (by id) an action; returns a function that removes it. */
export function registerAction<S extends ActionScope>(action: Action<S>): () => void {
  registry.set(action.id, action as unknown as Action);
  registryVersion++;
  listeners.forEach((l) => l());
  return () => {
    if (registry.get(action.id) === (action as unknown as Action)) {
      registry.delete(action.id);
      registryVersion++;
      listeners.forEach((l) => l());
    }
  };
}

/** Called whenever actions are added or removed, e.g. by the command palette. */
export function onActionsChanged(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The actions of `scope`, kept current in a component: re-rendered when actions come and go
 *  and when a shortcut changes in the settings. */
export function useActions<S extends ActionScope>(scope: S, arg: ActionScopes[S]): Action<S>[] {
  useSettingsVersion();
  useSyncExternalStore(onActionsChanged, () => registryVersion);
  return actionsFor(scope, arg);
}

/** Actions of a scope that apply to `arg`, in menu order: only the `multiple` ones for a
 *  selection of several rows. */
export function actionsFor<S extends ActionScope>(scope: S, arg: ActionScopes[S]): Action<S>[] {
  const many = ((arg as { selection?: unknown[] } | undefined)?.selection?.length ?? 0) > 1;
  return ([...registry.values()] as unknown as Action<S>[])
    .filter((a) => a.scope === scope && (!many || a.multiple) && (a.when?.(arg) ?? true))
    .sort((a, b) => (a.order ?? 100) - (b.order ?? 100));
}

/** Every registered action, for listings such as the command palette. */
export function allActions(): Action[] {
  return [...registry.values()];
}

/** The actions a shortcut can be set on. */
export function bindableActions(): Action[] {
  return allActions().filter((a) => !a.menuOnly);
}

export const isMac = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

/** Where a shortcut the user set is stored, by action id. */
const shortcutKey = (id: string) => `shortcuts.${id}`;

/** An action's shortcuts: the ones the user set (an empty string turns them off), else its
 *  own. */
export function shortcutsOf(action: Pick<Action, "id" | "shortcut">): string[] {
  const own = getStored(shortcutKey(action.id));
  if (typeof own === "string") return splitShortcuts(own);
  return [action.shortcut ?? []].flat();
}

/** Sets an action's shortcut: a combination, `""` for none, or `null` for its own. */
export function setShortcut(action: Pick<Action, "id">, shortcut: string | null): Promise<void> {
  return setStored(shortcutKey(action.id), shortcut);
}

/** Whether the user changed an action's shortcut. */
export function shortcutChanged(action: Pick<Action, "id">): boolean {
  return typeof getStored(shortcutKey(action.id)) === "string";
}

/** The combination of a key press as shortcuts write it, e.g. `Mod+Shift+K`; `null` for a
 *  modifier alone. */
export function shortcutOf(e: KeyboardEvent): string | null {
  if (["Control", "Shift", "Alt", "Meta", "AltGraph"].includes(e.key)) return null;
  const parts: string[] = [];
  if (isMac ? e.metaKey : e.ctrlKey) parts.push("Mod");
  if (isMac && e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  parts.push(keyName(e.key));
  return parts.join("+");
}

/** The action of `scope` whose shortcut is `e`, if any applies to `arg`: one written with the
 *  keys pressed, else one whose character needed Shift on this keyboard (`Mod+/` pressed as
 *  Ctrl+Shift+7 on an Italian one). */
export function actionForKey<S extends ActionScope>(
  scope: S,
  arg: ActionScopes[S],
  e: KeyboardEvent,
): Action<S> | undefined {
  const actions = actionsFor(scope, arg);
  const find = (loose: boolean) =>
    actions.find((a) => shortcutsOf(a).some((s) => matchesShortcut(s, e, isMac, loose)));
  return find(false) ?? (e.shiftKey ? find(true) : undefined);
}

/** While the Settings record a new shortcut, the keys go to them and run nothing. */
export const keyCapture = { active: false };

/** Runs the matching action and swallows the event; `true` if one ran. */
export function dispatchKey<S extends ActionScope>(
  scope: S,
  arg: ActionScopes[S],
  e: KeyboardEvent,
  onError: (e: unknown) => void,
): boolean {
  if (keyCapture.active) return false;
  const action = actionForKey(scope, arg, e);
  if (!action) return false;
  e.preventDefault();
  e.stopPropagation();
  // A disabled action keeps its keys: nothing else acts on them.
  runAction(action, arg, onError);
  return true;
}

/** Runs `action` on `arg`, unless it is disabled there. */
export function runAction<S extends ActionScope>(
  action: Action<S>,
  arg: ActionScopes[S],
  onError: (e: unknown) => void,
) {
  if (action.disabled?.(arg)) return;
  try {
    void Promise.resolve(action.run(arg)).catch(onError);
  } catch (e) {
    onError(e);
  }
}

/** Key names as keyboards of the interface's language print them, so menus, the palette, the
 *  Settings and the tooltips (translated texts) name keys alike. Not through `t`: "Delete" is
 *  also a command, translated otherwise. */
const KEY_LABELS: Record<string, Record<string, string>> = {
  it: {
    Shift: "Maiusc",
    Enter: "Invio",
    Delete: "Canc",
    Insert: "Ins",
    End: "Fine",
    Space: "Spazio",
    Escape: "Esc",
    PageUp: "PagSu",
    PageDown: "PagGiù",
  },
  es: {
    Shift: "Mayús",
    Enter: "Intro",
    Delete: "Supr",
    End: "Fin",
    Space: "Espacio",
    PageUp: "RePág",
    PageDown: "AvPág",
  },
  de: {
    Ctrl: "Strg",
    Shift: "Umschalt",
    Enter: "Eingabe",
    Delete: "Entf",
    Insert: "Einfg",
    End: "Ende",
    Space: "Leertaste",
    PageUp: "Bild↑",
    PageDown: "Bild↓",
  },
  fr: {
    Shift: "Maj",
    Enter: "Entrée",
    Delete: "Suppr",
    Insert: "Inser",
    End: "Fin",
    Space: "Espace",
    PageUp: "PgPréc",
    PageDown: "PgSuiv",
  },
  pt: {
    Space: "Espaço",
  },
};

/** One shortcut as shown: `Ctrl+Shift+T`, or `⌘⇧T` on macOS. */
export function shortcutText(shortcut: string): string {
  const names = KEY_LABELS[language()] ?? {};
  if (!isMac) return labelParts(shortcut, (k) => names[k] ?? k).join("+");
  const symbols: Record<string, string> = { Mod: "⌘", Shift: "⇧", Alt: "⌥", Ctrl: "⌃", Comma: ",", Plus: "+" };
  return shortcut
    .split("+")
    .map((p) => symbols[p] ?? p.replace("Arrow", "").toUpperCase())
    .join("");
}

/** `title` with the current shortcuts of action `id`, for a tooltip: `Close tab (Ctrl+W)`. */
export function titleWithKeys(title: string, id: string): string {
  const action = registry.get(id);
  const keys = action ? shortcutsOf(action).map(shortcutText).join(", ") : "";
  return keys ? `${title} (${keys})` : title;
}

/** The first shortcut as shown in menus: `Ctrl+Shift+T`, or `⌘⇧T` on macOS. */
export function shortcutLabel(action: Pick<Action, "id" | "shortcut">): string | undefined {
  const first = shortcutsOf(action)[0];
  return first ? shortcutText(first) : undefined;
}
