// The installed WebAssembly plugins, as Settings › Plugins, the driver picker and the action
// registry see them: read once at start and again after each change, in this window or another.
import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";
import { api, type InstalledPlugin, toCommandError } from "../api";
import { setPluginThemes } from "../theme";
import { changeFollower } from "./changeFollower";

let plugins: InstalledPlugin[] = [];
/** Why the list could not be read, the last time it was tried. */
let listProblem: string | null = null;
const listeners = new Set<() => void>();

/** Hears of plugins changed in another window, from the first time this one has a use for it: a
 *  plugin in the list, or a plugin's tab open. Without any, not even the listener is set up. */
export const followPluginChanges = changeFollower(
  (heard) => listen("plugins-changed", heard),
  () => reloadPlugins().catch((e: unknown) => console.warn("plugins", e)),
);

/** Called whenever the plugins change. */
export function onPluginsChanged(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function installedPlugins(): InstalledPlugin[] {
  return plugins;
}

export function pluginListProblem(): string | null {
  return listProblem;
}

function setPlugins(list: InstalledPlugin[]) {
  plugins = list;
  listProblem = null;
  if (list.length > 0) followPluginChanges();
  setPluginThemes(list.flatMap((p) => (p.enabled && p.manifest.theme ? [{ id: p.manifest.id, version: p.manifest.version, theme: p.manifest.theme }] : [])));
  listeners.forEach((l) => l());
  // What tells the user of a hook that failed, only once a plugin has hooks.
  if (list.some((p) => p.enabled && (p.manifest.hooks?.length || (p.broken && p.approved.hooks)))) {
    import("./hookNotices").catch((e: unknown) => console.warn("plugins", e));
  }
  // What their tabs, panels and grid extensions need, only once a plugin has one.
  if (list.some((p) => p.enabled && (p.manifest.tabs || p.manifest.panels || p.manifest.grid))) {
    import("./extensions").catch((e: unknown) => console.warn("plugins", e));
  }
}

/** Reads the plugins again from the data folder. */
export async function reloadPlugins(): Promise<InstalledPlugin[]> {
  try {
    setPlugins(await api.pluginsList());
  } catch (e) {
    // The appearance waits for this list when a plugin's theme is the one chosen.
    setPluginThemes([]);
    // No hook can run: whoever the first statement comes from, this is what hears of it.
    listProblem = toCommandError(e).message;
    import("./hookNotices").catch((failed: unknown) => console.warn("plugins", failed));
    throw e;
  }
  return plugins;
}

/** The plugins, kept current in a component. */
export function usePlugins(): InstalledPlugin[] {
  return useSyncExternalStore(onPluginsChanged, () => plugins);
}

interface PluginDriver {
  /** The plugin's id, the connection's `plugin` option. */
  id: string;
  name: string;
}

/** The drivers of the plugins that are turned on. */
export function pluginDrivers(list: InstalledPlugin[] = plugins): PluginDriver[] {
  return list.flatMap((p) =>
    p.enabled && p.description.driver ? [{ id: p.manifest.id, name: p.description.driver.name }] : [],
  );
}
