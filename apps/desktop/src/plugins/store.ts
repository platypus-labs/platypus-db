// The installed WebAssembly plugins, as Settings › Plugins, the driver picker and the action
// registry see them: read once at start and again after each change.
import { useSyncExternalStore } from "react";
import { api, type InstalledPlugin } from "../api";

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

export function setPlugins(list: InstalledPlugin[]) {
  plugins = list;
  listeners.forEach((l) => l());
}

/** Reads the plugins again from the data folder. */
export async function reloadPlugins(): Promise<InstalledPlugin[]> {
  setPlugins(await api.pluginsList());
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
