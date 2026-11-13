// Updates of the downloaded drivers: the settings, the updates found (shown as badges in
// Settings › Drivers), installing a version with the compatibility check that follows, and
// the check at startup.
import { createElement } from "react";
import { useSyncExternalStore } from "react";
import { api, DOWNLOADED_DRIVERS, type DriverKind, type DriverUpdate, type DriverUpdateCheck, type InstalledDriver } from "../api";
import { msg } from "../i18n";
import { defineSetting, getSetting } from "../settings";
import { workbench } from "../workbench";
import { CompatDialog } from "./CompatDialog";
import { compatTargets } from "./compat";

const SECTION = msg("Driver updates");

const checkUpdatesSetting = defineSetting<boolean>({
  key: "drivers.check_updates",
  section: SECTION,
  label: "Look for driver updates at startup",
  description: "Once a day at most, PlatypusDB asks the driver registry whether the downloaded drivers (Snowflake, BigQuery, Databricks, Trino) have a newer version.",
  default: true,
  control: { type: "boolean" },
});

const autoUpdateSetting = defineSetting<boolean>({
  key: "drivers.auto_update",
  section: SECTION,
  label: "Install driver updates by itself",
  description: "A newer version found at startup is downloaded and installed, keeping the previous one so that you can go back to it. Then it offers to check the connections that use the driver.",
  default: false,
  control: { type: "boolean" },
});

let updates: DriverUpdate[] = [];
const listeners = new Set<() => void>();

function setUpdates(next: DriverUpdate[]) {
  updates = next;
  listeners.forEach((l) => l());
}

/** The updates found by the last check, re-rendering when they change. */
export function useDriverUpdates(): DriverUpdate[] {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => updates,
  );
}

/** The `DriverKind` of a registry package name; `null` for one the app has no connection type for. */
export function kindOf(name: string): DriverKind | null {
  return DOWNLOADED_DRIVERS.find((k) => k === name) ?? null;
}

/** Asks the registry for newer versions of the installed drivers. Unless `force`, the backend
 *  skips the question when it was asked less than a day ago and answers with the updates the
 *  last check found (kept across restarts). */
export async function refreshUpdates(force: boolean): Promise<DriverUpdateCheck> {
  const result = await api.checkDriverUpdates(force);
  if (!result.error) setUpdates(result.updates);
  return result;
}

/** Downloads `version` of `driver` and makes it the active one; the previous one stays on disk.
 *  Returns the version that was active before, for the way back. */
export async function installVersion(
  driver: DriverKind,
  version: string,
  onProgress: (done: number, total: number | null) => void = () => {},
): Promise<{ installed: InstalledDriver; previous: string | null }> {
  const previous = (await api.listDrivers()).find((d) => d.name === driver)?.version ?? null;
  const installed = await api.installDriverVersion(driver, version, (e) => onProgress(e.done, e.total));
  setUpdates(updates.filter((u) => !(u.name === driver && u.package.version === installed.version)));
  return { installed, previous };
}

/** Opens the compatibility check of the connections that use `driver`, now at `to`; nothing
 *  opens when no saved connection uses it. `ask`: asks before connecting. */
export function checkCompatibility(driver: DriverKind, to: string, from: string | null, options: { ask: boolean; onChanged: () => void }) {
  const { run, skipped } = compatTargets(workbench().connections(), driver);
  if (run.length + skipped.length === 0) return;
  workbench().openDialog((close) =>
    createElement(CompatDialog, { driver, to, from, run, skipped, ask: options.ask, onChanged: options.onChanged, onClose: close }),
  );
}

/** At startup: looks for updates (at most once a day, unless the setting is off) and, with
 *  auto-update on, installs them, then offers the compatibility check. */
export async function startupDriverCheck() {
  if (!getSetting(checkUpdatesSetting)) return;
  let found: DriverUpdate[];
  try {
    const result = await refreshUpdates(false);
    // Also the updates of an earlier check: one not installed yet is retried at each start.
    found = result.error ? [] : result.updates;
  } catch (e) {
    console.info("driver update check:", e);
    return;
  }
  if (!getSetting(autoUpdateSetting)) return;
  for (const update of found) {
    const driver = kindOf(update.name);
    if (!driver) continue;
    try {
      const { installed, previous } = await installVersion(driver, update.package.version);
      checkCompatibility(driver, installed.version, previous, { ask: true, onChanged: () => {} });
    } catch (e) {
      console.warn("driver auto-update:", update.name, e);
    }
  }
}
