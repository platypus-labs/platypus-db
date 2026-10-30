/**
 * The sync and the shared projects while the app runs: the encrypted folder sync at start and
 * every few minutes, and the shared projects' folders when the window gets focus. Both can
 * also be asked for by hand (`runSync`, `refreshShared`); what they find is shown as a notice.
 */
import { api, toCommandError, type SharedNotice, type SyncReport } from "../api";
import { plural, t } from "../i18n";
import { scriptsChanged } from "../scripts/scripts";
import { loadSettings } from "../settings";
import { workbench } from "../workbench";

/** Puts the backend's sync errors in the interface's language, by their kind. */
export function syncErrorText(e: unknown): string {
  const error = toCommandError(e);
  switch (error.kind) {
    case "sync_passphrase":
      return t("Wrong sync passphrase, or the sync file is damaged. Nothing was changed.");
    case "sync_passphrase_empty":
      return t("Enter the sync passphrase.");
    case "sync_newer":
      return t("The sync file was written by a newer PlatypusDB. Update the app on this computer; nothing was changed.");
    case "sync_not_file":
      return t("The file platypus-sync.bin in that folder is not a PlatypusDB sync file.");
    case "sync_damaged":
      return t("The sync file is damaged. Nothing was changed.");
    case "sync_folder":
      return t("The sync folder cannot be used: {detail}", { detail: error.message });
    default:
      return error.message;
  }
}

/** Reloads what a sync or a shared folder changed in the app. */
export async function reloadWorkspace() {
  const [connections, projects, project_folders] = await Promise.all([api.listConnections(), api.listProjects(), api.listProjectFolders()]);
  workbench().setWorkspace({ connections, projects, project_folders });
  scriptsChanged();
}

const listeners = new Set<() => void>();

/** Calls `listener` after every sync run, successful or not. */
export function onSyncRan(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

let running: Promise<SyncReport> | null = null;

/** Runs the sync, once at a time: a second call waits for the one running. Reloads the app's
 *  data when something came in. */
export function runSync(): Promise<SyncReport> {
  running ??= (async () => {
    try {
      const report = await api.syncNow();
      if (report.applied > 0) {
        await loadSettings();
        await reloadWorkspace();
      }
      return report;
    } finally {
      running = null;
      listeners.forEach((l) => l());
    }
  })();
  return running;
}

/** What a run of the sync did, for a notice; `null` when there is nothing to say. */
export function syncSummary(report: SyncReport): string | null {
  if (report.conflicts.length > 0) {
    return plural(
      report.conflicts.length,
      "Sync: {n} change was made on both computers. Both versions were kept; see Settings, Sync.",
      "Sync: {n} changes were made on both computers. Both versions were kept; see Settings, Sync.",
    );
  }
  return report.applied > 0 ? plural(report.applied, "Sync: {n} change brought in.", "Sync: {n} changes brought in.") : null;
}

let lastError = "";

/** The sync as the background runs it: errors show once, not at every turn. */
async function backgroundSync() {
  try {
    const message = syncSummary(await runSync());
    lastError = "";
    if (message) workbench().notify(message);
  } catch (e) {
    const text = syncErrorText(e);
    if (text !== lastError) workbench().showError(`${t("Sync")}: ${text}`);
    lastError = text;
  }
}

let timer: ReturnType<typeof setTimeout> | null = null;
let stopped = false;

/** Schedules the next background run from the saved interval; call again after the settings
 *  of the Sync page change. */
export async function scheduleSync(first = false) {
  if (timer) clearTimeout(timer);
  timer = null;
  if (stopped) return;
  let minutes = 0;
  try {
    const status = await api.syncStatus();
    if (status.folder && status.has_passphrase) {
      minutes = status.interval_minutes;
      if (first && status.on_start) await backgroundSync();
    }
  } catch {
    // Nothing to schedule while the settings cannot be read.
  }
  if (stopped || minutes <= 0) return;
  timer = setTimeout(() => {
    void backgroundSync().then(() => scheduleSync());
  }, minutes * 60_000);
}

/** The notice for what reading a shared folder found, or `null`. */
export function sharedSummary(n: SharedNotice): string {
  switch (n.kind) {
    case "changed":
      return t("{name} was updated from its shared folder: {added} added, {removed} removed, {changed} changed.", {
        name: n.name,
        added: n.added,
        removed: n.removed,
        changed: n.changed,
      });
    case "missing":
      return t("The shared folder of {name} is not available: {detail}", { name: n.name, detail: n.message ?? "" });
    default:
      return t("The shared project file of {name} cannot be read, so it is left as it is: {detail}", { name: n.name, detail: n.message ?? "" });
  }
}

const shown = new Set<string>();

/** Reads the folders of the shared projects; changes come into the app with a notice. A
 *  problem is told once until it goes away. Returns the notices. */
export async function refreshShared(): Promise<SharedNotice[]> {
  const notices = await api.refreshSharedProjects();
  if (notices.some((n) => n.kind === "changed")) await reloadWorkspace();
  const current = new Set<string>();
  for (const n of notices) {
    const key = `${n.project}/${n.kind}/${n.message ?? ""}`;
    current.add(key);
    if (n.kind === "changed") workbench().notify(sharedSummary(n));
    else if (!shown.has(key)) workbench().showError(sharedSummary(n));
  }
  shown.clear();
  current.forEach((k) => shown.add(k));
  return notices;
}

/** Starts the background work; returns the function that stops it. */
export function startBackground(): () => void {
  stopped = false;
  void scheduleSync(true);
  const onFocus = () => void refreshShared().catch((e: unknown) => console.warn("shared projects", e));
  onFocus();
  window.addEventListener("focus", onFocus);
  return () => {
    stopped = true;
    if (timer) clearTimeout(timer);
    window.removeEventListener("focus", onFocus);
  };
}
