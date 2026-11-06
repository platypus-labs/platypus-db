// The installed WebAssembly plugins, as Settings › Plugins, the driver picker and the action
// registry see them: read once at start and again after each change.
import { useSyncExternalStore } from "react";
import { api, type InstalledPlugin } from "../api";
import { setPluginThemes } from "../theme";

let plugins: InstalledPlugin[] = [];
const listeners = new Set<() => void>();

/** Called whenever the plugins change. */
export function onPluginsChanged(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function installedPlugins(): InstalledPlugin[] {
  return plugins;
}

function setPlugins(list: InstalledPlugin[]) {
  plugins = list;
  setPluginThemes(list.flatMap((p) => (p.enabled && p.manifest.theme ? [{ id: p.manifest.id, theme: p.manifest.theme }] : [])));
  listeners.forEach((l) => l());
  // What tells the user of a hook that failed, only once a plugin has hooks.
  if (list.some((p) => p.enabled && p.manifest.hooks?.length)) {
    import("./hookNotices").catch((e: unknown) => console.warn("plugins", e));
  }
}

/** Reads the plugins again from the data folder. */
export async function reloadPlugins(): Promise<InstalledPlugin[]> {
  try {
    setPlugins(await api.pluginsList());
  } catch (e) {
    // The appearance waits for this list when a plugin's theme is the one chosen.
    setPluginThemes([]);
    throw e;
  }
  return plugins;
}

/** The plugins, kept current in a component. */
export function usePlugins(): InstalledPlugin[] {
  return useSyncExternalStore(onPluginsChanged, () => plugins);
}

export interface PluginDriver {
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
