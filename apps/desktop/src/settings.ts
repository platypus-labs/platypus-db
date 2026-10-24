/**
 * Typed user preferences. A feature declares its settings next to the code that reads
 * them with `defineSetting`; the Settings dialog lists every declared one by section.
 * Values are stored by the backend in `settings.toml` under the dotted key.
 */
import { useSyncExternalStore } from "react";
import { api } from "./api";

export type SettingControl =
  | { type: "boolean" }
  | { type: "number"; min?: number; max?: number; step?: number }
  | { type: "text"; placeholder?: string }
  | { type: "select"; options: { value: string; label: string }[] }
  /** A `#rrggbb` color; empty means none (e.g. the theme's own). */
  | { type: "color" }
  /** Choices shown as cards of colors, e.g. themes. */
  | { type: "swatches"; options: { value: string; label: string; colors: string[] }[] };

export interface SettingDef<T extends boolean | number | string> {
  /** Dotted, `section.name`, e.g. `editor.script_row_limit`. */
  key: string;
  /** Heading in the Settings dialog. */
  section: string;
  label: string;
  description?: string;
  default: T;
  control: SettingControl;
}

type AnyDef = SettingDef<boolean | number | string>;

const defs = new Map<string, AnyDef>();
/** Stored values by dotted key. */
let values = new Map<string, unknown>();
const listeners = new Set<() => void>();
let version = 0;

function changed() {
  version++;
  listeners.forEach((l) => l());
}

export function defineSetting<T extends boolean | number | string>(def: SettingDef<T>): SettingDef<T> {
  if (defs.has(def.key)) console.warn(`setting ${def.key} declared twice`);
  defs.set(def.key, def as AnyDef);
  return def;
}

export function settingDefs(): AnyDef[] {
  return [...defs.values()];
}

/** Loads stored values; call once before rendering. Failures leave the defaults. */
export async function loadSettings() {
  try {
    values = flatten(await api.getSettings());
    changed();
  } catch (e) {
    console.warn("settings", e);
  }
}

function flatten(tree: Record<string, unknown>, prefix = "", out = new Map<string, unknown>()) {
  for (const [k, v] of Object.entries(tree)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === "object" && !Array.isArray(v)) flatten(v as Record<string, unknown>, key, out);
    else out.set(key, v);
  }
  return out;
}

/** The stored value, or the default when unset or of another type (e.g. edited by hand). */
export function getSetting<T extends boolean | number | string>(def: SettingDef<T>): T {
  const v = values.get(def.key);
  return typeof v === typeof def.default ? (v as T) : def.default;
}

/** A stored value that no declared setting describes, e.g. a shortcut the user changed;
 *  `undefined` when unset. */
export function getStored(key: string): unknown {
  return values.get(key);
}

/** Stores such a value; `null` removes it. */
export async function setStored(key: string, value: string | number | boolean | null) {
  const previous = values.get(key);
  if (value === null) values.delete(key);
  else values.set(key, value);
  changed();
  try {
    await api.setSetting(key, value);
  } catch (e) {
    if (previous === undefined) values.delete(key);
    else values.set(key, previous);
    changed();
    throw e;
  }
}

/** Stores a value; `null` restores the default. */
export async function setSetting<T extends boolean | number | string>(def: SettingDef<T>, value: T | null) {
  const previous = values.get(def.key);
  if (value === null || value === def.default) values.delete(def.key);
  else values.set(def.key, value);
  changed();
  try {
    await api.setSetting(def.key, value === def.default ? null : value);
  } catch (e) {
    if (previous === undefined) values.delete(def.key);
    else values.set(def.key, previous);
    changed();
    throw e;
  }
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Calls `listener` after any setting changes; returns a function that stops it. */
export const onSettingsChanged = subscribe;

/** Re-renders when any setting changes. */
export function useSettingsVersion(): number {
  return useSyncExternalStore(subscribe, () => version);
}

export function useSetting<T extends boolean | number | string>(def: SettingDef<T>): T {
  useSettingsVersion();
  return getSetting(def);
}
